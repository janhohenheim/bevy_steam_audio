use std::num::NonZeroU32;

use crate::{AMBISONICS_NUM_CHANNELS, AMBISONICS_ORDER, FRAME_SIZE, prelude::*};

use bevy_seedling::{
    firewheel::diff::{Diff, Patch},
    node::RegisterNode as _,
    prelude::*,
};
use firewheel::{
    channel_config::ChannelConfig,
    event::ProcEvents,
    node::{
        AudioNode, AudioNodeInfo, AudioNodeProcessor, ConstructProcessorContext, EmptyConfig,
        ProcBuffers, ProcExtra, ProcInfo, ProcessStatus,
    },
};

pub(super) fn plugin(app: &mut App) {
    app.register_node::<AmbisonicDecodeNode>();
}

#[derive(Diff, Patch, Debug, Clone, Component)]
pub(crate) struct AmbisonicDecodeNode {
    pub(crate) listener_orientation: audionimbus::CoordinateSystem,
    #[diff(skip)]
    pub(crate) context: audionimbus::Context,
}

impl AmbisonicDecodeNode {
    pub(crate) fn new(context: audionimbus::Context) -> Self {
        Self {
            context,
            listener_orientation: default(),
        }
    }
}

impl AudioNode for AmbisonicDecodeNode {
    type Configuration = EmptyConfig;

    fn info(&self, _config: &Self::Configuration) -> AudioNodeInfo {
        AudioNodeInfo::new()
            .debug_name("ambisonic decode node")
            // 9 -> 2
            .channel_config(ChannelConfig {
                num_inputs: ChannelCount::new(AMBISONICS_NUM_CHANNELS).unwrap(),
                num_outputs: ChannelCount::STEREO,
            })
    }

    fn construct_processor(
        &self,
        _config: &Self::Configuration,
        cx: ConstructProcessorContext,
    ) -> impl AudioNodeProcessor {
        let settings = audionimbus::AudioSettings {
            sampling_rate: cx.stream_info.sample_rate.get(),
            frame_size: FRAME_SIZE,
        };
        let hrtf = audionimbus::Hrtf::try_new(
            &self.context,
            &settings,
            &audionimbus::HrtfSettings {
                volume_normalization: audionimbus::VolumeNormalization::RootMeanSquared,
                ..default()
            },
        )
        .unwrap();

        AmbisonicDecodeProcessor {
            params: self.clone(),
            hrtf: hrtf.clone(),
            ambisonics_decode_effect: audionimbus::AmbisonicsDecodeEffect::try_new(
                &self.context,
                &settings,
                &audionimbus::AmbisonicsDecodeEffectSettings {
                    max_order: AMBISONICS_ORDER,
                    speaker_layout: audionimbus::SpeakerLayout::Stereo,
                    hrtf: &hrtf,
                },
            )
            .unwrap(),
            input_buffer: std::array::from_fn(|_| Vec::with_capacity(FRAME_SIZE as usize)),
            output_buffer: std::array::from_fn(|_| {
                Vec::with_capacity(cx.stream_info.max_block_frames.get() as usize * 2)
            }),
            max_block_frames: cx.stream_info.max_block_frames,
            started_draining: false,
        }
    }
}

struct AmbisonicDecodeProcessor {
    params: AmbisonicDecodeNode,
    hrtf: audionimbus::Hrtf,
    ambisonics_decode_effect: audionimbus::AmbisonicsDecodeEffect,
    input_buffer: [Vec<f32>; AMBISONICS_NUM_CHANNELS as usize],
    output_buffer: [Vec<f32>; 2],
    max_block_frames: NonZeroU32,
    started_draining: bool,
}

impl AudioNodeProcessor for AmbisonicDecodeProcessor {
    fn process(
        &mut self,
        proc_info: &ProcInfo,
        ProcBuffers { inputs, outputs }: ProcBuffers,
        events: &mut ProcEvents,
        _: &mut ProcExtra,
    ) -> ProcessStatus {
        for patch in events.drain_patches::<AmbisonicDecodeNode>() {
            self.params.apply(patch);
        }

        if proc_info.in_silence_mask.all_channels_silent(inputs.len()) {
            return ProcessStatus::ClearAllOutputs;
        }

        for frame in 0..proc_info.frames {
            for (dst, src) in self.input_buffer.iter_mut().zip(inputs) {
                dst.push(src[frame]);
            }
            if self.input_buffer[0].len() != self.input_buffer[0].capacity() {
                continue;
            }
            // Buffer full

            let mut mix_container = [0.0; AMBISONICS_NUM_CHANNELS as usize * FRAME_SIZE as usize];
            for channel in 0..AMBISONICS_NUM_CHANNELS as usize {
                mix_container[(channel * FRAME_SIZE as usize)..(channel + 1) * FRAME_SIZE as usize]
                    .copy_from_slice(&self.input_buffer[channel]);
            }
            let mut channel_ptrs = [std::ptr::null_mut(); AMBISONICS_NUM_CHANNELS as usize];
            let settings = audionimbus::AudioBufferSettings {
                num_channels: Some(AMBISONICS_NUM_CHANNELS),
                ..default()
            };
            let mix_buffer = audionimbus::AudioBuffer::try_borrowed_with_data_and_settings(
                &mut mix_container,
                &mut channel_ptrs,
                settings,
            )
            .unwrap();

            let mut staging_container = [0.0; FRAME_SIZE as usize * 2];
            let mut channel_ptrs = [std::ptr::null_mut(); 2];
            let settings = audionimbus::AudioBufferSettings {
                num_channels: Some(outputs.len() as u32),
                ..default()
            };
            let staging_buffer = audionimbus::AudioBuffer::try_borrowed_with_data_and_settings(
                &mut staging_container,
                &mut channel_ptrs,
                settings,
            )
            .unwrap();

            let ambisonics_decode_effect_params = audionimbus::AmbisonicsDecodeEffectParams {
                order: AMBISONICS_ORDER,
                hrtf: &self.hrtf,
                orientation: self.params.listener_orientation,
                binaural: true,
            };
            let _effect_state = self.ambisonics_decode_effect.apply(
                &ambisonics_decode_effect_params,
                &mix_buffer,
                &staging_buffer,
            );

            let left = &staging_container[..FRAME_SIZE as usize];
            let right = &staging_container[FRAME_SIZE as usize..];
            self.output_buffer[0].extend(left);
            self.output_buffer[1].extend(right);
            for buff in &mut self.input_buffer {
                buff.clear();
            }
        }

        for buff in &self.input_buffer {
            if buff.capacity() > FRAME_SIZE as usize {
                error!("allocated input_buffer in processor, this is a bug");
            }
        }

        for buff in &self.output_buffer {
            if buff.capacity() > self.max_block_frames.get() as usize * 2 {
                error!("allocated output_buffer in processor, this is a bug");
            }
        }

        if !self.started_draining {
            if (self.output_buffer[0].len() as f32) < self.max_block_frames.get() as f32 * 1.5 {
                return ProcessStatus::ClearAllOutputs;
            }
            self.started_draining = true;
        }

        let output_len = outputs[0].len();
        for (src, dst) in self.output_buffer.iter_mut().zip(outputs.iter_mut()) {
            for (i, out) in src.drain(..output_len).enumerate() {
                dst[i] = out;
            }
        }
        ProcessStatus::OutputsModified
    }
}
