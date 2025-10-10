use std::{iter, num::NonZeroU32};

use crate::{
    FRAME_SIZE, GAIN_FACTOR_DIRECT, GAIN_FACTOR_REFLECTIONS, GAIN_FACTOR_REVERB,
    STEAM_AUDIO_CONTEXT, nodes::reverb::SharedReverbData, prelude::*,
    settings::order_to_num_channels, simulation::SimulationOutputEvent, wrapper::ChannelPtrs,
};

use audionimbus::AudioBuffer;
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
use itertools::izip;

pub(super) fn plugin(app: &mut App) {
    app.register_node::<AudionimbusNode>();
}
#[derive(Diff, Patch, Debug, Default, PartialEq, Clone, RealtimeClone, Component, Reflect)]
#[reflect(Component)]
pub struct AudionimbusNode {
    pub(crate) source_position: Vec3,
    pub(crate) listener_position: Vec3,
}

#[derive(Diff, Patch, Debug, Clone, RealtimeClone, PartialEq, Component, Reflect)]
#[reflect(Component)]
pub struct AudionimbusNodeConfig {
    pub(crate) order: u32,
}

impl Default for AudionimbusNodeConfig {
    fn default() -> Self {
        Self { order: 2 }
    }
}

impl AudionimbusNodeConfig {
    #[inline]
    fn num_channels(&self) -> u32 {
        order_to_num_channels(self.order)
    }
}

impl AudioNode for AudionimbusNode {
    type Configuration = AudionimbusNodeConfig;

    fn info(&self, config: &Self::Configuration) -> AudioNodeInfo {
        AudioNodeInfo::new()
            .debug_name("ambisonic node")
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
            frame_size: FRAME_SIZE,
        };
        AudionimbusProcessor {
            params: self.clone(),
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
            input_buffer: Vec::with_capacity(FRAME_SIZE as usize),
            output_buffer: iter::repeat_n(
                Vec::with_capacity(cx.stream_info.max_block_frames.get() as usize * 2),
                config.num_channels() as usize,
            )
            .collect(),
            max_block_frames: cx.stream_info.max_block_frames,
            started_draining: false,
            simulation_outputs: None,
            order: config.order,
            ambisonics_encode_container: vec![0.0; (FRAME_SIZE * config.num_channels()) as usize],
            ambisonics_encode_ptrs: vec![std::ptr::null_mut(); config.num_channels() as usize]
                .into(),
            reflections_container: vec![0.0; (FRAME_SIZE * config.num_channels()) as usize],
            reflections_ptrs: vec![std::ptr::null_mut(); config.num_channels() as usize].into(),
            reverb_container: vec![0.0; (FRAME_SIZE * config.num_channels()) as usize],
            reverb_ptrs: vec![std::ptr::null_mut(); config.num_channels() as usize].into(),
        }
    }
}

struct AudionimbusProcessor {
    order: u32,
    params: AudionimbusNode,
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
}

impl AudioNodeProcessor for AudionimbusProcessor {
    fn process(
        &mut self,
        proc_info: &ProcInfo,
        ProcBuffers { inputs, outputs }: ProcBuffers,
        events: &mut ProcEvents,
        extra: &mut ProcExtra,
    ) -> ProcessStatus {
        for mut event in events.drain() {
            if let Some(patch) = AudionimbusNode::patch_event(&event) {
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
            let mut input_container = [0.0; FRAME_SIZE as usize];
            input_container.copy_from_slice(&self.input_buffer);
            let input_buffer = audionimbus::AudioBuffer::try_borrowed_with_data(
                &input_container,
                &mut channel_ptrs,
            )
            .unwrap();

            let mut direct_container = [0.0; FRAME_SIZE as usize];
            let mut channel_ptrs = [std::ptr::null_mut(); 1];
            let direct_buffer = audionimbus::AudioBuffer::try_borrowed_with_data(
                &mut direct_container,
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
                    (direct_sample * GAIN_FACTOR_DIRECT
                        + reflections_sample * GAIN_FACTOR_REFLECTIONS
                        + reverb_sample * GAIN_FACTOR_REVERB)
                        / (GAIN_FACTOR_DIRECT + GAIN_FACTOR_REFLECTIONS + GAIN_FACTOR_REVERB)
                })
            })
            .enumerate()
            .for_each(|(i, channel)| {
                self.output_buffer[i].extend(channel);
            });
            self.input_buffer.clear();
        }

        if self.input_buffer.capacity() > FRAME_SIZE as usize {
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
