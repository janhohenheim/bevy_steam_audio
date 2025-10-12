use crate::{nodes::reverb::ReverbDataNode, prelude::*, settings::SteamAudioQuality};
use bevy_seedling::prelude::*;
use core::iter;
use firewheel::node::{ProcBuffers, ProcInfo, ProcessStatus};
use prealloc_ref_vec::{PreallocRefVec, TmpRefVec};

pub(crate) mod decoder;
pub(crate) mod encoder;
pub(crate) mod reverb;

pub use decoder::*;
pub use encoder::*;

pub(super) fn plugin(app: &mut App) {
    app.add_systems(PreStartup, setup_nodes);
    app.add_plugins((decoder::plugin, encoder::plugin, reverb::plugin));
    app.register_required_components::<SteamAudioPool, Transform>()
        .register_required_components::<SteamAudioPool, GlobalTransform>()
        .register_required_components::<SteamAudioPool, SteamAudioSamplePlayer>();
}

#[derive(PoolLabel, PartialEq, Eq, Debug, Hash, Clone, Default)]
pub struct SteamAudioPool;

#[derive(NodeLabel, PartialEq, Eq, Debug, Hash, Clone)]
pub struct SteamAudioDecodeBus;

pub(crate) fn setup_nodes(mut commands: Commands, quality: Res<SteamAudioQuality>) {
    // we only need one decoder
    commands.spawn((SteamAudioDecodeBus, SteamAudioDecodeNode::default()));
    commands.spawn(ReverbDataNode);

    // Copy-paste this part if you want to set up your own pool!
    commands
        .spawn((
            SamplerPool(SteamAudioPool),
            VolumeNodeConfig {
                channels: NonZeroChannelCount::new(quality.num_channels()).unwrap(),
            },
            sample_effects![SteamAudioNode::default()],
        ))
        .connect(SteamAudioDecodeBus);
}

/// A helper to encapsulate processing audio in fixed blocks.
struct FixedProcessBlock {
    inputs: FlatChannels,
    // The outputs are represented as a collection of `Vec`
    // because pusing beyond the expected bounds has a
    // non-zero chance of happening.
    outputs: Box<[Vec<f32>]>,
    input_slices: PreallocRefVec<[f32]>,
    output_slices: PreallocRefVec<[f32]>,
}

impl FixedProcessBlock {
    pub fn new(
        fixed_block_size: usize,
        max_output_size: usize,
        input_channels: usize,
        output_channels: usize,
    ) -> Self {
        let inputs = FlatChannels::new(input_channels, fixed_block_size);

        let outputs = iter::repeat_with(|| {
            let mut vec = Vec::new();
            vec.reserve_exact(max_output_size);
            vec
        })
        .take(output_channels)
        .collect();

        Self {
            inputs,
            outputs,
            input_slices: PreallocRefVec::new(input_channels),
            output_slices: PreallocRefVec::new(output_channels),
        }
    }

    pub fn resize(&mut self, fixed_block_size: usize, max_output_size: usize) {
        // otherwise, extend and copy as needed
        if let Some(output) = self.outputs.first()
            && output.capacity() != max_output_size
        {
            for buffer in self.outputs.iter_mut() {
                if max_output_size > buffer.capacity() {
                    buffer.reserve_exact(max_output_size - buffer.capacity());
                } else {
                    buffer.resize(max_output_size, 0f32);
                    buffer.shrink_to_fit();
                }
            }
        }

        if fixed_block_size != self.inputs.channel_capacity {
            // these we have to completely rebuild
            let mut new_inputs = FlatChannels::new(self.inputs.channel_count, fixed_block_size);

            for channel in 0..self.inputs.channel_count {
                for frame in 0..self.inputs.length {
                    *new_inputs.get_mut(channel, frame) = *self.inputs.get(channel, frame);
                }
            }

            self.inputs = new_inputs;
        }
    }

    pub fn process<F>(
        &mut self,
        buffers: ProcBuffers,
        info: &ProcInfo,
        mut process: F,
    ) -> ProcessStatus
    where
        F: FnMut(&[&[f32]], &mut [&mut [f32]]),
    {
        let mut processed_frames = 0;
        let mut remaining_inner_capacity = self.inputs.remaining_capacity();
        let mut next_process_event = if info.frames >= remaining_inner_capacity {
            Some(remaining_inner_capacity)
        } else {
            None
        };

        while let Some(event_index) = next_process_event {
            let range = processed_frames..event_index;

            // The surrounding code essentially facilitates this. Rather
            // then pushing one sample at a time to each channel, checking
            // if the inputs are full, we pre-calculate exactly how many samples
            // we need to fill the inputs. This allows us to copy over the data
            // in bulk, which should be significantly more efficient.
            for (inner_buffer, outer_buffer) in
                self.inputs.iter_remaining_capacity().zip(buffers.inputs)
            {
                inner_buffer.copy_from_slice(&outer_buffer[range.clone()]);
            }
            self.inputs.length = self.inputs.channel_capacity;

            let mut temp_inputs = self.input_slices.get_tmp();
            self.inputs.fill_slices(&mut temp_inputs);

            let mut temp_outputs = self.output_slices.get_tmp_mut();
            for output in self.outputs.iter_mut() {
                let start = output.len();
                output.extend(iter::repeat_n(0f32, self.inputs.channel_capacity));

                temp_outputs.push(&mut output[start..]);
            }

            process(&temp_inputs, &mut temp_outputs);
            drop((temp_inputs, temp_outputs));

            self.inputs.clear();
            processed_frames = range.end;

            remaining_inner_capacity = self.inputs.remaining_capacity();
            let remaining_outer_capacity = info.frames - processed_frames;
            next_process_event = if remaining_outer_capacity >= remaining_inner_capacity {
                Some(processed_frames + remaining_inner_capacity)
            } else {
                None
            };
        }

        // push remaining data
        if processed_frames != info.frames {
            let remaining_frames = info.frames - processed_frames;
            for (inner_buffer, outer_buffer) in
                self.inputs.iter_remaining_capacity().zip(buffers.inputs)
            {
                let inner_buffer = &mut inner_buffer[..remaining_frames];
                inner_buffer.copy_from_slice(&outer_buffer[processed_frames..]);
            }

            self.inputs.length += remaining_frames;
        }

        // write outputs
        if let Some(inner_buffer) = self.outputs.first()
            && let Some(outer_buffer) = buffers.outputs.first()
            && inner_buffer.len() >= outer_buffer.len()
        {
            for (proc_out, buffer) in buffers.outputs.iter_mut().zip(&mut self.outputs) {
                let buffer_len = buffer.len();
                for (i, sample) in buffer.drain(..proc_out.len().min(buffer_len)).enumerate() {
                    proc_out[i] = sample;
                }
            }

            return ProcessStatus::OutputsModified;
        }

        ProcessStatus::ClearAllOutputs
    }

    pub fn frame_size(&self) -> usize {
        self.inputs.channel_capacity
    }

    pub fn max_block_frames(&self) -> usize {
        self.outputs.first()
            .map(Vec::capacity)
            .unwrap_or_default()
    }

    pub fn inputs_clear(&self) -> bool {
        self.inputs.length == 0
    }

    pub fn clear(&mut self) {
        self.inputs.clear();
        for output in self.outputs.iter_mut() {
            output.clear();
        }
    }
}

/// A set of channels allocated as one slab.
///
/// Resizing is infrequent, so the extra stack size,
/// potential unused capacity, and potential heap
/// fragmentation of nested vectors is undesirable.
struct FlatChannels {
    data: Box<[f32]>,
    channel_count: usize,
    channel_capacity: usize,
    length: usize,
}

impl FlatChannels {
    fn new(channel_count: usize, channel_capacity: usize) -> Self {
        let total_len = channel_count * channel_capacity;
        let data = iter::repeat_n(0f32, total_len).collect();

        Self {
            data,
            channel_count,
            channel_capacity,
            length: 0,
        }
    }

    fn index(&self, channel: usize, frame: usize) -> usize {
        self.channel_capacity * channel + frame
    }

    fn get(&self, channel: usize, frame: usize) -> &f32 {
        &self.data[self.index(channel, frame)]
    }

    fn get_mut(&mut self, channel: usize, frame: usize) -> &mut f32 {
        &mut self.data[self.index(channel, frame)]
    }

    fn iter_remaining_capacity(&mut self) -> impl Iterator<Item = &'_ mut [f32]> {
        let length = self.length;
        self.data
            .chunks_mut(self.channel_capacity)
            .map(move |slice| &mut slice[length..])
    }

    fn remaining_capacity(&self) -> usize {
        self.channel_capacity - self.length
    }

    fn clear(&mut self) {
        self.length = 0;
    }

    fn fill_slices<'a>(&'a self, lens: &mut TmpRefVec<'a, [f32]>) {
        for channel in 0..self.channel_count {
            let start = self.index(channel, 0);
            let len = self.length;

            lens.push(&self.data[start..start + len]);
        }
    }
}
