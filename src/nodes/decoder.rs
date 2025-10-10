use std::{iter, num::NonZeroU32};

use crate::{
    FRAME_SIZE, STEAM_AUDIO_CONTEXT,
    prelude::*,
    settings::order_to_num_channels,
    wrapper::{AudionimbusCoordinateSystem, ChannelPtrs},
};

use bevy_seedling::{
    firewheel::diff::{Diff, Patch},
    node::RegisterNode as _,
    prelude::*,
};
use firewheel::{
    channel_config::ChannelConfig,
    diff::RealtimeClone,
    event::ProcEvents,
    node::{
        AudioNode, AudioNodeInfo, AudioNodeProcessor, ConstructProcessorContext, EmptyConfig,
        ProcBuffers, ProcExtra, ProcInfo, ProcessStatus,
    },
};

pub(super) fn plugin(app: &mut App) {
    app.register_node::<AmbisonicDecodeNode>();
}

#[derive(Diff, Patch, Debug, Default, PartialEq, Clone, RealtimeClone, Component, Reflect)]
#[reflect(Component)]
pub struct AmbisonicDecodeNode {
    pub(crate) listener_orientation: AudionimbusCoordinateSystem,
}

#[derive(Diff, Patch, Debug, Clone, RealtimeClone, PartialEq, Component)]
pub struct AmbisonicDecodeNodeConfig {
    pub(crate) order: u32,
}

impl Default for AmbisonicDecodeNodeConfig {
    fn default() -> Self {
        Self { order: 2 }
    }
}

impl AmbisonicDecodeNodeConfig {
    pub(crate) fn num_channels(&self) -> u32 {
        order_to_num_channels(self.order)
    }
}

impl AudioNode for AmbisonicDecodeNode {
    type Configuration = AmbisonicDecodeNodeConfig;

    fn info(&self, config: &Self::Configuration) -> AudioNodeInfo {
        AudioNodeInfo::new()
            .debug_name("ambisonic decode node")
            // 9 -> 2
            .channel_config(ChannelConfig {
                num_inputs: ChannelCount::new(order_to_num_channels(config.order)).unwrap(),
                num_outputs: ChannelCount::STEREO,
            })
    }

    fn construct_processor(
        &self,
        config: &Self::Configuration,
        cx: ConstructProcessorContext,
    ) -> impl AudioNodeProcessor {
        let settings = audionimbus::AudioSettings {
            sampling_rate: cx.stream_info.sample_rate.get(),
            frame_size: FRAME_SIZE,
        };
        let hrtf = audionimbus::Hrtf::try_new(
            &STEAM_AUDIO_CONTEXT,
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
                &STEAM_AUDIO_CONTEXT,
                &settings,
                &audionimbus::AmbisonicsDecodeEffectSettings {
                    max_order: config.order,
                    speaker_layout: audionimbus::SpeakerLayout::Stereo,
                    hrtf: &hrtf,
                },
            )
            .unwrap(),
            input_buffer: iter::repeat_n(
                Vec::with_capacity(FRAME_SIZE as usize),
                config.num_channels() as usize,
            )
            .collect(),
            output_buffer: std::array::from_fn(|_| {
                Vec::with_capacity(cx.stream_info.max_block_frames.get() as usize * 2)
            }),
            max_block_frames: cx.stream_info.max_block_frames,
            started_draining: false,
            order: config.order,
            mix_container: vec![0.0; (FRAME_SIZE * config.num_channels()) as usize],
            mix_ptrs: vec![std::ptr::null_mut(); config.num_channels() as usize].into(),
        }
    }
}

struct AmbisonicDecodeProcessor {
    params: AmbisonicDecodeNode,
    hrtf: audionimbus::Hrtf,
    ambisonics_decode_effect: audionimbus::AmbisonicsDecodeEffect,
    input_buffer: Vec<Vec<f32>>,
    output_buffer: [Vec<f32>; 2],
    max_block_frames: NonZeroU32,
    started_draining: bool,
    order: u32,
    mix_container: Vec<f32>,
    mix_ptrs: ChannelPtrs,
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
            Patch::apply(&mut self.params, patch);
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
            let channels = order_to_num_channels(self.order);

            for channel in 0..channels as usize {
                self.mix_container
                    [(channel * FRAME_SIZE as usize)..(channel + 1) * FRAME_SIZE as usize]
                    .copy_from_slice(&self.input_buffer[channel]);
            }
            let settings = audionimbus::AudioBufferSettings {
                num_channels: Some(channels),
                ..default()
            };
            let mix_buffer = audionimbus::AudioBuffer::try_borrowed_with_data_and_settings(
                &mut self.mix_container,
                &mut self.mix_ptrs,
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
                order: self.order,
                hrtf: &self.hrtf,
                orientation: self.params.listener_orientation.to_audionimbus(),
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
