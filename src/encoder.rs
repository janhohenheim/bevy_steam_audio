use std::num::NonZeroU32;

use crate::{
    AMBISONICS_NUM_CHANNELS, AMBISONICS_ORDER, FRAME_SIZE, GAIN_FACTOR_DIRECT,
    GAIN_FACTOR_REFLECTIONS, GAIN_FACTOR_REVERB, prelude::*,
};

use bevy_seedling::{
    firewheel::diff::{Diff, Patch},
    node::RegisterNode as _,
    prelude::*,
};
use firewheel::{
    channel_config::ChannelConfig,
    collector::{ArcGc, OwnedGc},
    event::{NodeEventType, ProcEvents},
    node::{
        AudioNode, AudioNodeInfo, AudioNodeProcessor, ConstructProcessorContext, EmptyConfig,
        ProcBuffers, ProcExtra, ProcInfo, ProcessStatus,
    },
};
use itertools::izip;

pub(super) fn plugin(app: &mut App) {
    app.register_node::<AudionimbusNode>();
}

#[derive(Diff, Patch, Debug, Clone, Component)]
pub(crate) struct AudionimbusNode {
    pub(crate) source_position: Vec3,
    pub(crate) listener_position: Vec3,
    #[diff(skip)]
    pub(crate) context: audionimbus::Context,
}

impl AudionimbusNode {
    pub(crate) fn new(context: audionimbus::Context) -> Self {
        Self {
            context,
            source_position: default(),
            listener_position: default(),
        }
    }
}

impl AudioNode for AudionimbusNode {
    type Configuration = EmptyConfig;

    fn info(&self, _config: &Self::Configuration) -> AudioNodeInfo {
        AudioNodeInfo::new()
            .debug_name("ambisonic node")
            // 1 -> 9
            .channel_config(ChannelConfig {
                num_inputs: ChannelCount::MONO,
                num_outputs: ChannelCount::new(AMBISONICS_NUM_CHANNELS).unwrap(),
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
        AudionimbusProcessor {
            params: self.clone(),
            ambisonics_encode_effect: audionimbus::AmbisonicsEncodeEffect::try_new(
                &self.context,
                &settings,
                &audionimbus::AmbisonicsEncodeEffectSettings {
                    max_order: AMBISONICS_ORDER,
                },
            )
            .unwrap(),
            direct_effect: audionimbus::DirectEffect::try_new(
                &self.context,
                &settings,
                &audionimbus::DirectEffectSettings { num_channels: 1 },
            )
            .unwrap(),
            reflection_effect: audionimbus::ReflectionEffect::try_new(
                &self.context,
                &settings,
                &audionimbus::ReflectionEffectSettings::Convolution {
                    impulse_response_size: 2 * settings.sampling_rate,
                    num_channels: AMBISONICS_NUM_CHANNELS,
                },
            )
            .unwrap(),
            reverb_effect: audionimbus::ReflectionEffect::try_new(
                &self.context,
                &settings,
                &audionimbus::ReflectionEffectSettings::Convolution {
                    impulse_response_size: 2 * settings.sampling_rate,
                    num_channels: AMBISONICS_NUM_CHANNELS,
                },
            )
            .unwrap(),
            input_buffer: Vec::with_capacity(FRAME_SIZE as usize),
            output_buffer: std::array::from_fn(|_| {
                Vec::with_capacity(cx.stream_info.max_block_frames.get() as usize * 2)
            }),
            max_block_frames: cx.stream_info.max_block_frames,
            started_draining: false,
            simulation_outputs: None,
            reverb_effect_params: None,
        }
    }
}

pub(crate) struct SimulationUpdate {
    pub(crate) outputs: Option<audionimbus::SimulationOutputs>,
    pub(crate) reverb_effect_params: ArcGc<OwnedGc<audionimbus::ReflectionEffectParams>>,
}

struct AudionimbusProcessor {
    params: AudionimbusNode,
    ambisonics_encode_effect: audionimbus::AmbisonicsEncodeEffect,
    direct_effect: audionimbus::DirectEffect,
    reflection_effect: audionimbus::ReflectionEffect,
    reverb_effect: audionimbus::ReflectionEffect,
    input_buffer: Vec<f32>,
    output_buffer: [Vec<f32>; AMBISONICS_NUM_CHANNELS as usize],
    max_block_frames: NonZeroU32,
    started_draining: bool,
    simulation_outputs: Option<audionimbus::SimulationOutputs>,
    reverb_effect_params: Option<ArcGc<OwnedGc<audionimbus::ReflectionEffectParams>>>,
}

impl AudioNodeProcessor for AudionimbusProcessor {
    fn process(
        &mut self,
        proc_info: &ProcInfo,
        ProcBuffers { inputs, outputs }: ProcBuffers,
        events: &mut ProcEvents,
        _: &mut ProcExtra,
    ) -> ProcessStatus {
        for event in events.drain() {
            if let Some(patch) = AudionimbusNode::patch_event(&event) {
                self.params.apply(patch);
            }
            if let NodeEventType::Custom(mut event) = event
                && let Some(update) = event.get_mut().downcast_mut::<SimulationUpdate>()
            {
                if let Some(outputs) = update.outputs.take() {
                    self.simulation_outputs = Some(outputs);
                }
                self.reverb_effect_params = Some(update.reverb_effect_params.clone());
            }
        }

        // Don't early return on silent inputs: there is probably reverb left

        for frame in inputs[0].iter().take(proc_info.frames).copied() {
            self.input_buffer.push(frame);
            if self.input_buffer.len() != self.input_buffer.capacity() {
                continue;
            }
            // Buffer full, let's work!

            let (Some(simulation_outputs), Some(reverb_effect_params)) = (
                self.simulation_outputs.as_ref(),
                self.reverb_effect_params.as_ref(),
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

            let mut ambisonics_encode_container =
                [0.0; (FRAME_SIZE * AMBISONICS_NUM_CHANNELS) as usize];
            let settings = audionimbus::AudioBufferSettings {
                num_channels: Some(AMBISONICS_NUM_CHANNELS),
                ..default()
            };
            let mut channel_ptrs = [std::ptr::null_mut(); AMBISONICS_NUM_CHANNELS as usize];
            let ambisonics_encode_buffer =
                audionimbus::AudioBuffer::try_borrowed_with_data_and_settings(
                    &mut ambisonics_encode_container,
                    &mut channel_ptrs,
                    settings,
                )
                .unwrap();
            let ambisonics_encode_effect_params = audionimbus::AmbisonicsEncodeEffectParams {
                direction,
                order: AMBISONICS_ORDER,
            };
            let _effect_state = self.ambisonics_encode_effect.apply(
                &ambisonics_encode_effect_params,
                &direct_buffer,
                &ambisonics_encode_buffer,
            );

            let mut reflection_container = [0.0; (FRAME_SIZE * AMBISONICS_NUM_CHANNELS) as usize];
            let settings = audionimbus::AudioBufferSettings {
                num_channels: Some(AMBISONICS_NUM_CHANNELS),
                ..default()
            };
            let mut channel_ptrs = [std::ptr::null_mut(); AMBISONICS_NUM_CHANNELS as usize];
            let reflection_buffer = audionimbus::AudioBuffer::try_borrowed_with_data_and_settings(
                &mut reflection_container,
                &mut channel_ptrs,
                settings,
            )
            .unwrap();
            let _effect_state = self.reflection_effect.apply(
                &reflection_effect_params,
                &input_buffer,
                &reflection_buffer,
            );

            let mut reverb_container = [0.0; (FRAME_SIZE * AMBISONICS_NUM_CHANNELS) as usize];
            let settings = audionimbus::AudioBufferSettings {
                num_channels: Some(AMBISONICS_NUM_CHANNELS),
                ..default()
            };
            let mut channel_ptrs = [std::ptr::null_mut(); AMBISONICS_NUM_CHANNELS as usize];
            let reverb_buffer = audionimbus::AudioBuffer::try_borrowed_with_data_and_settings(
                &mut reverb_container,
                &mut channel_ptrs,
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
