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

    // TODO: this is likely much less efficient than extending
    // each input vec by the correct amount in one go.
    fn push_frame(&mut self, input_frame: usize, inputs: &[&[f32]]) {
        for (channel, buffer) in inputs.iter().enumerate() {
            *self.get_mut(channel, self.length) = buffer[input_frame];
        }
        self.length += 1;
    }

    fn is_full(&self) -> bool {
        self.length == self.channel_capacity
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

struct FixedProcessBlock {
    inputs: FlatChannels,
    outputs: Box<[Vec<f32>]>,
    input_lens: PreallocRefVec<[f32]>,
    output_lens: PreallocRefVec<[f32]>,
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
            input_lens: PreallocRefVec::new(input_channels),
            output_lens: PreallocRefVec::new(output_channels),
        }
    }

    pub fn resize(&mut self, fixed_block_size: usize, max_output_size: usize) {
        // otherwise, extend and copy as needed
        if let Some(output) = self.outputs.get(0)
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
        for input_frame in 0..info.frames {
            self.inputs.push_frame(input_frame, buffers.inputs);
            if self.inputs.is_full() {
                let mut temp_inputs = self.input_lens.get_tmp();
                self.inputs.fill_slices(&mut temp_inputs);

                let mut temp_outputs = self.output_lens.get_tmp_mut();
                for output in self.outputs.iter_mut() {
                    let start = output.len();
                    output.extend(iter::repeat_n(0f32, self.inputs.channel_capacity));

                    temp_outputs.push(&mut output[start..]);
                }

                process(&temp_inputs, &mut temp_outputs);
                drop((temp_inputs, temp_outputs));

                self.inputs.clear();
            }
        }

        if let Some(inner_buffer) = self.outputs.get(0)
            && let Some(outer_buffer) = buffers.outputs.get(0)
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
