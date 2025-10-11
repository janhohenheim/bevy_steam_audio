use std::{iter, num::NonZeroU32};

use crate::{
    STEAM_AUDIO_CONTEXT,
    prelude::*,
    settings::{SteamAudioQuality, order_to_num_channels},
    wrapper::{AudionimbusCoordinateSystem, ChannelPtrs},
};

use bevy_ecs::{lifecycle::HookContext, world::DeferredWorld};
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
        AudioNode, AudioNodeInfo, AudioNodeProcessor, ConstructProcessorContext, ProcBuffers,
        ProcExtra, ProcInfo, ProcessStatus,
    },
};

pub(super) fn plugin(app: &mut App) {
    app.register_node::<SteamAudioDecodeNode>();
}

#[derive(Diff, Patch, Debug, Default, PartialEq, Clone, RealtimeClone, Component, Reflect)]
#[reflect(Component)]
pub struct SteamAudioDecodeNode {
    pub(crate) listener_orientation: AudionimbusCoordinateSystem,
}

#[derive(Diff, Patch, Debug, Clone, RealtimeClone, PartialEq, Default, Component, Reflect)]
#[reflect(Component)]
#[component(on_add = on_add_decode_node_config)]
pub struct SteamAudioDecodeNodeConfig {
    pub(crate) order: u32,
    pub(crate) frame_size: u32,
}

fn on_add_decode_node_config(mut world: DeferredWorld, ctx: HookContext) {
    let quality = *world.resource::<SteamAudioQuality>();
    let mut entity = world.entity_mut(ctx.entity);
    let mut config = entity.get_mut::<SteamAudioDecodeNodeConfig>().unwrap();
    config.order = quality.order;
    config.frame_size = quality.frame_size;
}

impl SteamAudioDecodeNodeConfig {
    pub(crate) fn num_channels(&self) -> u32 {
        order_to_num_channels(self.order)
    }
}

impl AudioNode for SteamAudioDecodeNode {
    type Configuration = SteamAudioDecodeNodeConfig;

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
            frame_size: config.frame_size,
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

        SteamAudioDecodeProcessor {
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
            input_buffer: iter::repeat_with(|| Vec::with_capacity(config.frame_size as usize))
                .take(config.num_channels() as usize)
                .collect(),
            output_buffer: std::array::from_fn(|_| {
                Vec::with_capacity(cx.stream_info.max_block_frames.get() as usize * 2)
            }),
            max_block_frames: cx.stream_info.max_block_frames,
            started_draining: false,
            order: config.order,
            frame_size: config.frame_size,
            mix_container: vec![0.0; (config.frame_size * config.num_channels()) as usize],
            staging_container: vec![0.0; (config.frame_size * 2) as usize],
            mix_ptrs: vec![std::ptr::null_mut(); config.num_channels() as usize].into(),
        }
    }
}

struct SteamAudioDecodeProcessor {
    params: SteamAudioDecodeNode,
    hrtf: audionimbus::Hrtf,
    ambisonics_decode_effect: audionimbus::AmbisonicsDecodeEffect,
    input_buffer: Vec<Vec<f32>>,
    output_buffer: [Vec<f32>; 2],
    max_block_frames: NonZeroU32,
    started_draining: bool,
    order: u32,
    frame_size: u32,
    mix_container: Vec<f32>,
    staging_container: Vec<f32>,
    mix_ptrs: ChannelPtrs,
}

impl SteamAudioDecodeProcessor {
    #[inline]
    fn num_channels(&self) -> u32 {
        order_to_num_channels(self.order)
    }

    #[inline]
    fn total_capacity(&self) -> usize {
        [&self.mix_container, &self.staging_container]
            .iter()
            .map(|b| b.capacity())
            .chain(iter::once(self.input_buffer.capacity()))
            .chain(self.input_buffer.iter().map(|b| b.capacity()))
            .chain(self.output_buffer.iter().map(|b| b.capacity()))
            .sum()
    }

    #[inline]
    fn validate_capacity(&self, start_capacity: usize) {
        let end_capacity = self.total_capacity();
        if start_capacity != end_capacity {
            warn!(
                "Allocated in SteamAudioDecodeProcessor. Capacity mismatch: {} != {}",
                start_capacity, end_capacity
            );
        }
    }
}

impl AudioNodeProcessor for SteamAudioDecodeProcessor {
    fn process(
        &mut self,
        proc_info: &ProcInfo,
        ProcBuffers { inputs, outputs }: ProcBuffers,
        events: &mut ProcEvents,
        _: &mut ProcExtra,
    ) -> ProcessStatus {
        let start_capacity = self.total_capacity();
        for patch in events.drain_patches::<SteamAudioDecodeNode>() {
            Patch::apply(&mut self.params, patch);
        }

        if proc_info.in_silence_mask.all_channels_silent(inputs.len()) {
            self.validate_capacity(start_capacity);
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
            let channels = self.num_channels();

            for channel in 0..channels as usize {
                self.mix_container[(channel * self.frame_size as usize)
                    ..(channel + 1) * self.frame_size as usize]
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

            let mut channel_ptrs = [std::ptr::null_mut(); 2];
            let settings = audionimbus::AudioBufferSettings {
                num_channels: Some(outputs.len() as u32),
                ..default()
            };
            let staging_buffer = audionimbus::AudioBuffer::try_borrowed_with_data_and_settings(
                &mut self.staging_container,
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

            let left = &self.staging_container[..self.frame_size as usize];
            let right = &self.staging_container[self.frame_size as usize..];
            self.output_buffer[0].extend(left);
            self.output_buffer[1].extend(right);
            for buff in &mut self.input_buffer {
                buff.clear();
            }
        }

        if !self.started_draining {
            if (self.output_buffer[0].len() as f32) < self.max_block_frames.get() as f32 * 1.5 {
                self.validate_capacity(start_capacity);
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
        self.validate_capacity(start_capacity);
        ProcessStatus::OutputsModified
    }

    fn new_stream(
        &mut self,
        stream_info: &firewheel::StreamInfo,
        _context: &mut firewheel::node::ProcStreamCtx,
    ) {
        let settings = audionimbus::AudioSettings {
            sampling_rate: stream_info.sample_rate.get(),
            frame_size: self.frame_size,
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

        self.hrtf = hrtf.clone();
        self.ambisonics_decode_effect = audionimbus::AmbisonicsDecodeEffect::try_new(
            &STEAM_AUDIO_CONTEXT,
            &settings,
            &audionimbus::AmbisonicsDecodeEffectSettings {
                max_order: self.order,
                speaker_layout: audionimbus::SpeakerLayout::Stereo,
                hrtf: &hrtf,
            },
        )
        .unwrap();
        for (i, old) in self.output_buffer.clone().into_iter().enumerate() {
            let mut vec = Vec::with_capacity(stream_info.max_block_frames.get() as usize * 2);
            vec.extend(old);
            self.output_buffer[i] = vec;
        }
        self.max_block_frames = stream_info.max_block_frames;
    }
}
