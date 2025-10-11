use std::{iter, num::NonZeroU32};

use crate::{
    STEAM_AUDIO_CONTEXT,
    nodes::reverb::SharedReverbData,
    prelude::*,
    settings::{SteamAudioQuality, order_to_num_channels},
    simulation::SimulationOutputEvent,
    wrapper::ChannelPtrs,
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
use itertools::izip;

pub(super) fn plugin(app: &mut App) {
    app.register_node::<SteamAudioNode>();
}
#[derive(Diff, Patch, Debug, PartialEq, Clone, RealtimeClone, Component, Reflect)]
#[reflect(Component)]
pub struct SteamAudioNode {
    pub direct_gain: f32,
    pub reflection_gain: f32,
    pub reverb_gain: f32,
    pub(crate) source_position: Vec3,
    pub(crate) listener_position: Vec3,
}

impl Default for SteamAudioNode {
    fn default() -> Self {
        Self {
            direct_gain: 1.0,
            reflection_gain: 0.5,
            reverb_gain: 0.02,
            source_position: Vec3::ZERO,
            listener_position: Vec3::ZERO,
        }
    }
}

#[derive(Diff, Patch, Debug, Clone, RealtimeClone, PartialEq, Component, Default, Reflect)]
#[reflect(Component)]
#[component(on_add = on_add_audionimbus_node_config)]
pub struct SteamAUdioNodeConfig {
    pub(crate) order: u32,
    pub(crate) frame_size: u32,
}

fn on_add_audionimbus_node_config(mut world: DeferredWorld, ctx: HookContext) {
    let quality = *world.resource::<SteamAudioQuality>();
    let mut entity = world.entity_mut(ctx.entity);
    let mut config = entity.get_mut::<SteamAUdioNodeConfig>().unwrap();
    config.order = quality.order;
    config.frame_size = quality.frame_size;
}

impl SteamAUdioNodeConfig {
    #[inline]
    fn num_channels(&self) -> u32 {
        order_to_num_channels(self.order)
    }
}

impl AudioNode for SteamAudioNode {
    type Configuration = SteamAUdioNodeConfig;

    fn info(&self, config: &Self::Configuration) -> AudioNodeInfo {
        AudioNodeInfo::new()
            .debug_name("Steam Audio node")
            // 1 -> 9
            .channel_config(ChannelConfig {
                num_inputs: ChannelCount::MONO,
                num_outputs: ChannelCount::new(config.num_channels()).unwrap(),
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
        SteamAudioProcessor {
            params: self.clone(),
            frame_size: config.frame_size,
            ambisonics_encode_effect: audionimbus::AmbisonicsEncodeEffect::try_new(
                &STEAM_AUDIO_CONTEXT,
                &settings,
                &audionimbus::AmbisonicsEncodeEffectSettings {
                    max_order: config.order,
                },
            )
            .unwrap(),
            direct_effect: audionimbus::DirectEffect::try_new(
                &STEAM_AUDIO_CONTEXT,
                &settings,
                &audionimbus::DirectEffectSettings { num_channels: 1 },
            )
            .unwrap(),
            reflection_effect: audionimbus::ReflectionEffect::try_new(
                &STEAM_AUDIO_CONTEXT,
                &settings,
                &audionimbus::ReflectionEffectSettings::Convolution {
                    impulse_response_size: 2 * settings.sampling_rate,
                    num_channels: config.num_channels(),
                },
            )
            .unwrap(),
            reverb_effect: audionimbus::ReflectionEffect::try_new(
                &STEAM_AUDIO_CONTEXT,
                &settings,
                &audionimbus::ReflectionEffectSettings::Convolution {
                    impulse_response_size: 2 * settings.sampling_rate,
                    num_channels: config.num_channels(),
                },
            )
            .unwrap(),
            input_buffer: Vec::with_capacity(config.frame_size as usize),

            output_buffer: iter::repeat_with(|| {
                Vec::with_capacity(cx.stream_info.max_block_frames.get() as usize * 2)
            })
            .take(config.num_channels() as usize)
            .collect(),
            max_block_frames: cx.stream_info.max_block_frames,
            started_draining: false,
            simulation_outputs: None,
            order: config.order,
            ambisonics_encode_container: vec![
                0.0;
                (config.frame_size * config.num_channels()) as usize
            ],
            ambisonics_encode_ptrs: vec![std::ptr::null_mut(); config.num_channels() as usize]
                .into(),
            reflections_container: vec![0.0; (config.frame_size * config.num_channels()) as usize],
            reflections_ptrs: vec![std::ptr::null_mut(); config.num_channels() as usize].into(),
            reverb_container: vec![0.0; (config.frame_size * config.num_channels()) as usize],
            reverb_ptrs: vec![std::ptr::null_mut(); config.num_channels() as usize].into(),
            input_container: vec![0.0; (config.frame_size) as usize],
            direct_container: vec![0.0; (config.frame_size) as usize],
        }
    }
}

struct SteamAudioProcessor {
    order: u32,
    frame_size: u32,
    params: SteamAudioNode,
    ambisonics_encode_effect: audionimbus::AmbisonicsEncodeEffect,
    direct_effect: audionimbus::DirectEffect,
    reflection_effect: audionimbus::ReflectionEffect,
    reverb_effect: audionimbus::ReflectionEffect,
    input_buffer: Vec<f32>,
    output_buffer: Vec<Vec<f32>>,
    max_block_frames: NonZeroU32,
    started_draining: bool,
    simulation_outputs: Option<audionimbus::SimulationOutputs>,
    ambisonics_encode_container: Vec<f32>,
    ambisonics_encode_ptrs: ChannelPtrs,
    reflections_container: Vec<f32>,
    reflections_ptrs: ChannelPtrs,
    reverb_container: Vec<f32>,
    reverb_ptrs: ChannelPtrs,
    input_container: Vec<f32>,
    direct_container: Vec<f32>,
}

impl AudioNodeProcessor for SteamAudioProcessor {
    fn process(
        &mut self,
        proc_info: &ProcInfo,
        ProcBuffers { inputs, outputs }: ProcBuffers,
        events: &mut ProcEvents,
        extra: &mut ProcExtra,
    ) -> ProcessStatus {
        for mut event in events.drain() {
            if let Some(patch) = SteamAudioNode::patch_event(&event) {
                Patch::apply(&mut self.params, patch);
            }
            if let Some(update) = event.downcast::<SimulationOutputEvent>() {
                self.simulation_outputs = Some(update.0);
            }
        }

        // Don't early return on silent inputs: there is probably reverb left

        for frame in inputs[0].iter().take(proc_info.frames).copied() {
            self.input_buffer.push(frame);
            if self.input_buffer.len() != self.input_buffer.capacity() {
                continue;
            }
            // Buffer full, let's work!

            let (Some(simulation_outputs), Some(SharedReverbData(reverb_effect_params))) = (
                self.simulation_outputs.as_ref(),
                extra.store.try_get::<SharedReverbData>(),
            ) else {
                self.input_buffer.clear();
                return ProcessStatus::ClearAllOutputs;
            };

            let source_position = self.params.source_position;

            let direct_effect_params = simulation_outputs.direct();
            let reflection_effect_params = simulation_outputs.reflections();

            let mut channel_ptrs = [std::ptr::null_mut(); 1];
            self.input_container.copy_from_slice(&self.input_buffer);
            let input_buffer = audionimbus::AudioBuffer::try_borrowed_with_data(
                &self.input_container,
                &mut channel_ptrs,
            )
            .unwrap();

            let mut channel_ptrs = [std::ptr::null_mut(); 1];
            let direct_buffer = audionimbus::AudioBuffer::try_borrowed_with_data(
                &mut self.direct_container,
                &mut channel_ptrs,
            )
            .unwrap();
            let _effect_state = self.direct_effect.apply(
                &direct_effect_params.clone(),
                &input_buffer,
                &direct_buffer,
            );

            let listener_position = self.params.listener_position;
            let direction = source_position - listener_position;
            let direction = audionimbus::Direction::new(direction.x, direction.y, direction.z);

            let settings = audionimbus::AudioBufferSettings {
                num_channels: Some(order_to_num_channels(self.order)),
                ..default()
            };
            let ambisonics_encode_buffer =
                audionimbus::AudioBuffer::try_borrowed_with_data_and_settings(
                    &mut self.ambisonics_encode_container,
                    &mut self.ambisonics_encode_ptrs,
                    settings,
                )
                .unwrap();
            let ambisonics_encode_effect_params = audionimbus::AmbisonicsEncodeEffectParams {
                direction,
                order: self.order,
            };
            let _effect_state = self.ambisonics_encode_effect.apply(
                &ambisonics_encode_effect_params,
                &direct_buffer,
                &ambisonics_encode_buffer,
            );

            let reflection_buffer = audionimbus::AudioBuffer::try_borrowed_with_data_and_settings(
                &mut self.reflections_container,
                &mut self.reflections_ptrs,
                settings,
            )
            .unwrap();
            let _effect_state = self.reflection_effect.apply(
                &reflection_effect_params,
                &input_buffer,
                &reflection_buffer,
            );

            let reverb_buffer = audionimbus::AudioBuffer::try_borrowed_with_data_and_settings(
                &mut self.reverb_container,
                &mut self.reverb_ptrs,
                settings,
            )
            .unwrap();

            let _effect_state =
                self.reverb_effect
                    .apply(reverb_effect_params, &input_buffer, &reverb_buffer);

            izip!(
                ambisonics_encode_buffer.channels(),
                reflection_buffer.channels(),
                reverb_buffer.channels()
            )
            .map(|(direct_channel, reflection_channel, reverb_channel)| {
                izip!(
                    direct_channel.iter(),
                    reflection_channel.iter(),
                    reverb_channel.iter()
                )
                .map(|(direct_sample, reflections_sample, reverb_sample)| {
                    direct_sample * self.params.direct_gain
                        + reflections_sample * self.params.reflection_gain
                        + reverb_sample * self.params.reverb_gain
                })
            })
            .enumerate()
            .for_each(|(i, channel)| {
                self.output_buffer[i].extend(channel);
            });
            self.input_buffer.clear();
        }

        if self.input_buffer.capacity() > self.frame_size as usize {
            error!("allocated input_buffer in processor, this is a bug");
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

        let output_len = proc_info.frames;
        for (src, dst) in self.output_buffer.iter_mut().zip(outputs.iter_mut()) {
            for (i, out) in src.drain(..output_len).enumerate() {
                dst[i] = out;
            }
        }
        ProcessStatus::OutputsModified
    }
}
