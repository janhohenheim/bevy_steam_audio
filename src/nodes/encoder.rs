use crate::{
    STEAM_AUDIO_CONTEXT,
    nodes::FixedProcessBlock,
    prelude::*,
    settings::{SteamAudioQuality, order_to_num_channels},
    wrapper::{AudionimbusCoordinateSystem, ChannelPtrs},
};

use audionimbus::AudioBuffer;
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
    app.register_node::<SteamAudioNode>();
}
#[derive(Diff, Patch, Debug, PartialEq, Clone, RealtimeClone, Component, Reflect)]
#[reflect(Component)]
pub struct SteamAudioNode {
    pub direct_gain: f32,
    pub reflection_gain: f32,
    pub pathing_gain: f32,

    pub previous_direct_gain: f32,
    pub previous_reflection_gain: f32,
    pub previous_pathing_gain: f32,
    pub source_position: Vec3,
    pub listener_position: AudionimbusCoordinateSystem,
    pub pathing_available: bool,
}

impl Default for SteamAudioNode {
    fn default() -> Self {
        Self {
            direct_gain: 1.0,
            reflection_gain: 1.0,
            pathing_gain: 1.0,

            previous_direct_gain: 0.0,
            previous_reflection_gain: 0.0,
            previous_pathing_gain: 0.0,
            source_position: Vec3::ZERO,
            listener_position: AudionimbusCoordinateSystem::default(),
            pathing_available: false,
        }
    }
}

#[derive(Debug, Clone, RealtimeClone, PartialEq, Component, Default)]
#[component(on_add = on_add_steam_audio_node_config)]
pub struct SteamAudioNodeConfig {
    pub(crate) hrtf: Option<audionimbus::Hrtf>,
    pub(crate) quality: SteamAudioQuality,
}

fn on_add_steam_audio_node_config(mut world: DeferredWorld, ctx: HookContext) {
    let quality = *world.resource::<SteamAudioQuality>();
    let mut entity = world.entity_mut(ctx.entity);
    let mut config = entity.get_mut::<SteamAudioNodeConfig>().unwrap();
    config.quality = quality;
}

impl SteamAudioNodeConfig {
    #[inline]
    fn num_channels(&self) -> u32 {
        order_to_num_channels(self.quality.order)
    }
}

impl AudioNode for SteamAudioNode {
    type Configuration = SteamAudioNodeConfig;

    fn info(&self, _config: &Self::Configuration) -> AudioNodeInfo {
        AudioNodeInfo::new()
            .debug_name("Steam Audio node")
            .channel_config(ChannelConfig {
                num_inputs: ChannelCount::STEREO,
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
            frame_size: config.quality.frame_size,
        };
        let hrtf = config.hrtf.clone().expect("Created an `AudioNode` before the audio stream was ready. Please wait until `SteamAudioReady` is triggered.");
        SteamAudioProcessor {
            params: self.clone(),
            ambisonics_encode_effect: audionimbus::AmbisonicsEncodeEffect::try_new(
                &STEAM_AUDIO_CONTEXT,
                &settings,
                &audionimbus::AmbisonicsEncodeEffectSettings {
                    max_order: config.quality.order,
                },
            )
            .unwrap(),
            direct_effect: audionimbus::DirectEffect::try_new(
                &STEAM_AUDIO_CONTEXT,
                &settings,
                &audionimbus::DirectEffectSettings { num_channels: 2 },
            )
            .unwrap(),
            reflection_effect: audionimbus::ReflectionEffect::try_new(
                &STEAM_AUDIO_CONTEXT,
                &settings,
                &audionimbus::ReflectionEffectSettings::Convolution {
                    impulse_response_size: impulse_response_size(
                        config.quality,
                        settings.sampling_rate,
                    ),
                    num_channels: config.num_channels(),
                },
            )
            .unwrap(),
            binaural_effect: audionimbus::BinauralEffect::try_new(
                &STEAM_AUDIO_CONTEXT,
                &settings,
                &audionimbus::BinauralEffectSettings { hrtf: &hrtf },
            )
            .unwrap(),
            pathing_effect: audionimbus::PathEffect::try_new(
                &STEAM_AUDIO_CONTEXT,
                &settings,
                &audionimbus::PathEffectSettings {
                    max_order: config.quality.order,
                    spatialization: Some(audionimbus::Spatialization {
                        speaker_layout: audionimbus::SpeakerLayout::Stereo,
                        hrtf: &hrtf,
                    }),
                },
            )
            .unwrap(),
            ambisonics_decode_effect: audionimbus::AmbisonicsDecodeEffect::try_new(
                &STEAM_AUDIO_CONTEXT,
                &settings,
                &audionimbus::AmbisonicsDecodeEffectSettings {
                    max_order: config.quality.order,
                    speaker_layout: audionimbus::SpeakerLayout::Stereo,
                    hrtf: &hrtf,
                },
            )
            .unwrap(),
            fixed_block: FixedProcessBlock::new(
                config.quality.frame_size as usize,
                cx.stream_info.max_block_frames.get() as usize,
                2,
                2,
            ),
            direct_effect_params: None,
            reflection_effect_params: None,
            pathing_effect_params: None,
            quality: config.quality,
            hrtf,
            ambisonics_ptrs: ChannelPtrs::new(config.num_channels() as usize),
            ambisonics_buffer: core::iter::repeat_n(
                0f32,
                (config.quality.frame_size * config.num_channels()) as usize,
            )
            .collect(),
        }
    }
}

struct SteamAudioProcessor {
    quality: SteamAudioQuality,
    params: SteamAudioNode,
    ambisonics_encode_effect: audionimbus::AmbisonicsEncodeEffect,
    direct_effect: audionimbus::DirectEffect,
    reflection_effect: audionimbus::ReflectionEffect,
    binaural_effect: audionimbus::BinauralEffect,
    pathing_effect: audionimbus::PathEffect,
    ambisonics_decode_effect: audionimbus::AmbisonicsDecodeEffect,
    fixed_block: FixedProcessBlock,
    direct_effect_params: Option<audionimbus::DirectEffectParams>,
    reflection_effect_params: Option<audionimbus::ReflectionEffectParams>,
    pathing_effect_params: Option<audionimbus::PathEffectParams>,
    // We might be able to use the scratch buffers for this, but
    // the ambisonic order may produce more channels than scratch
    // buffers.
    ambisonics_buffer: Box<[f32]>,
    ambisonics_ptrs: ChannelPtrs,
    hrtf: audionimbus::Hrtf,
}

impl SteamAudioProcessor {
    #[inline]
    fn num_channels(&self) -> u32 {
        order_to_num_channels(self.quality.order)
    }
}

impl AudioNodeProcessor for SteamAudioProcessor {
    fn process(
        &mut self,
        proc_info: &ProcInfo,
        proc_buffers: ProcBuffers,
        events: &mut ProcEvents,
        extra: &mut ProcExtra,
    ) -> ProcessStatus {
        for mut event in events.drain() {
            if let Some(patch) = SteamAudioNode::patch_event(&event) {
                Patch::apply(&mut self.params, patch);
            }
            if let Some(update) = event.downcast::<SimulationOutputEvent>() {
                if self.direct_effect_params.is_none()
                    || update.flags.contains(audionimbus::SimulationFlags::DIRECT)
                {
                    self.direct_effect_params = Some(update.outputs.direct().into_inner());
                }
                if self.reflection_effect_params.is_none()
                    || update
                        .flags
                        .contains(audionimbus::SimulationFlags::REFLECTIONS)
                {
                    self.reflection_effect_params = Some(update.outputs.reflections().into_inner());
                }
                if self.pathing_effect_params.is_none()
                    || update.flags.contains(audionimbus::SimulationFlags::PATHING)
                {
                    self.pathing_effect_params = Some(update.outputs.pathing().into_inner());
                }
            }
        }

        // If the previous output of this node was silent, and the inputs are also silent
        // then we know there is no reverb tail left and we can skip processing.
        if proc_info.prev_output_was_silent && proc_info.in_silence_mask.all_channels_silent(2) {
            return ProcessStatus::ClearAllOutputs;
        }

        let (
            Some(direct_effect_params),
            Some(reflection_effect_params),
            Some(pathing_effect_params),
        ) = (
            self.direct_effect_params.as_ref(),
            self.reflection_effect_params.as_mut(),
            self.pathing_effect_params.as_mut(),
        )
        else {
            // If this is encountered at any point other than just
            // after insertion into the graph, then something's gone
            // quite wrong. So, we'll clear the fixed buffers.
            self.fixed_block.clear();
            return ProcessStatus::ClearAllOutputs;
        };

        let [
            scratch_stereo_left,
            scratch_stereo_right,
            scratch_mono_reflect,
            scratch_mono_pathing,
        ] = extra.scratch_buffers.channels_mut();
        let frame_size = self.fixed_block.frame_size();

        let fixed_block = &mut self.fixed_block;
        let temp_proc = ProcBuffers {
            inputs: proc_buffers.inputs,
            outputs: proc_buffers.outputs,
        };
        let result = fixed_block.process(temp_proc, proc_info, |inputs, outputs| {
            let source_position = self.params.source_position;
            let listener = self.params.listener_position;

            assert_eq!(inputs[0].len(), frame_size);
            let mut channel_ptrs = [inputs[0].as_ptr().cast_mut(), inputs[1].as_ptr().cast_mut()];

            // SAFETY:
            // `channel_ptrs` points to `frame_size` floats, whose lifetime
            // will outlast `input_sa_buffer`.
            let input_sa_buffer = unsafe {
                AudioBuffer::<&[f32], _>::try_new_borrowed(
                    channel_ptrs.as_mut_slice(),
                    frame_size as u32,
                )
                .unwrap()
            };

            assert_eq!(outputs[0].len(), frame_size);
            let mut channel_ptrs = [
                outputs[0].as_ptr().cast_mut(),
                outputs[1].as_ptr().cast_mut(),
            ];

            // SAFETY:
            // `channel_ptrs` points to `frame_size` floats, whose lifetime
            // will outlast `output_sa_buffer`.
            let mut output_sa_buffer = unsafe {
                AudioBuffer::<&mut [f32], _>::try_new_borrowed(
                    channel_ptrs.as_mut_slice(),
                    frame_size as u32,
                )
                .unwrap()
            };

            assert!(scratch_mono_reflect.len() >= frame_size);
            let mut channel_ptrs = [scratch_mono_reflect.as_mut_ptr()];
            // SAFETY:
            // `channel_ptrs` points to `frame_size` floats, whose lifetime
            // will outlast `mono_sa_buffer`.
            let mut mono_reflect_sa_buffer = unsafe {
                AudioBuffer::<&mut [f32], _>::try_new_borrowed(
                    channel_ptrs.as_mut_slice(),
                    frame_size as u32,
                )
                .unwrap()
            };
            mono_reflect_sa_buffer.downmix(&STEAM_AUDIO_CONTEXT, &input_sa_buffer);

            assert!(scratch_mono_pathing.len() >= frame_size);
            let mut channel_ptrs = [scratch_mono_pathing.as_mut_ptr()];
            // SAFETY:
            // `channel_ptrs` points to `frame_size` floats, whose lifetime
            // will outlast `mono_sa_buffer`.
            let mut mono_pathing_sa_buffer = unsafe {
                AudioBuffer::<&mut [f32], _>::try_new_borrowed(
                    channel_ptrs.as_mut_slice(),
                    frame_size as u32,
                )
                .unwrap()
            };
            mono_pathing_sa_buffer.downmix(&STEAM_AUDIO_CONTEXT, &input_sa_buffer);

            assert!(scratch_stereo_left.len() >= frame_size);
            assert!(scratch_stereo_right.len() >= frame_size);
            let mut channel_ptrs = [
                scratch_stereo_left.as_mut_ptr(),
                scratch_stereo_right.as_mut_ptr(),
            ];

            // Direct Effect

            // SAFETY:
            //
            // `channel_ptrs` points to `frame_size` floats, whose lifetime
            // will outlast `direct_sa_buffer`.
            let scratch_stereo_sa_buffer = unsafe {
                AudioBuffer::<&mut [f32], _>::try_new_borrowed(
                    channel_ptrs.as_mut_slice(),
                    frame_size as u32,
                )
                .unwrap()
            };

            let _effect_state = self.direct_effect.apply(
                &direct_effect_params.clone(),
                &input_sa_buffer,
                &scratch_stereo_sa_buffer,
            );

            // Binaural Effect
            let direction = source_position - listener.origin;
            let direction = audionimbus::Direction::new(direction.x, direction.y, direction.z);
            let binaural_params = audionimbus::BinauralEffectParams {
                direction,
                interpolation: audionimbus::HrtfInterpolation::Bilinear,
                spatial_blend: 1.0,
                hrtf: &self.hrtf,
                peak_delays: None,
            };

            let _effect_state = self.binaural_effect.apply(
                &binaural_params,
                &scratch_stereo_sa_buffer,
                &output_sa_buffer,
            );
            apply_volume_ramp(
                self.params.previous_direct_gain,
                self.params.direct_gain,
                outputs,
            );
            self.params.previous_direct_gain = self.params.direct_gain;

            // Reflection effect
            let settings = audionimbus::AudioBufferSettings {
                num_channels: Some(self.quality.num_channels()),
                frame_size: Some(frame_size as u32),
                ..default()
            };
            let ambisonics_sa_buffer = AudioBuffer::try_borrowed_with_data_and_settings(
                &mut self.ambisonics_buffer,
                &mut self.ambisonics_ptrs,
                settings,
            )
            .unwrap();
            reflection_effect_params.reflection_effect_type =
                audionimbus::ReflectionEffectType::Convolution;
            reflection_effect_params.num_channels = self.quality.num_channels();
            reflection_effect_params.impulse_response_size =
                impulse_response_size(self.quality, proc_info.sample_rate.into());

            apply_volume_ramp(
                self.params.previous_reflection_gain,
                self.params.reflection_gain,
                &mut [scratch_mono_reflect],
            );
            self.params.previous_reflection_gain = self.params.reflection_gain;

            let _effect_state = self.reflection_effect.apply(
                reflection_effect_params,
                &mono_reflect_sa_buffer,
                &ambisonics_sa_buffer,
            );

            // Decode ambisonics
            let ambisonics_decode_effect_params = audionimbus::AmbisonicsDecodeEffectParams {
                order: self.quality.order,
                hrtf: &self.hrtf,
                orientation: listener.to_audionimbus(),
                binaural: true,
            };
            let _effect_state = self.ambisonics_decode_effect.apply(
                &ambisonics_decode_effect_params,
                &ambisonics_sa_buffer,
                &scratch_stereo_sa_buffer,
            );

            output_sa_buffer.mix(&STEAM_AUDIO_CONTEXT, &scratch_stereo_sa_buffer);

            // Pathing effect
            if self.params.pathing_available {
                pathing_effect_params.order = self.quality.order;
                pathing_effect_params.listener = listener.to_audionimbus();
                pathing_effect_params.binaural = true;
                pathing_effect_params.hrtf = self.hrtf.clone();
                for coeff in &mut pathing_effect_params.eq_coeffs {
                    *coeff = coeff.max(0.1)
                }

                apply_volume_ramp(
                    self.params.previous_pathing_gain,
                    self.params.pathing_gain,
                    &mut [scratch_mono_reflect],
                );
                self.params.previous_pathing_gain = self.params.pathing_gain;
                let _effect_state = self.pathing_effect.apply(
                    pathing_effect_params,
                    &mono_pathing_sa_buffer,
                    &scratch_stereo_sa_buffer,
                );

                output_sa_buffer.mix(&STEAM_AUDIO_CONTEXT, &scratch_stereo_sa_buffer);
            }
        });

        // check for silence when the input is silent
        if matches!(result, ProcessStatus::OutputsModified)
            && proc_info.in_silence_mask.all_channels_silent(2)
        {
            proc_buffers.check_for_silence_on_outputs(0.0001)
        } else {
            result
        }
    }

    fn new_stream(
        &mut self,
        stream_info: &firewheel::StreamInfo,
        _context: &mut firewheel::node::ProcStreamCtx,
    ) {
        // If these parameter don't change, there's no need to thrash the audio state.
        if stream_info.sample_rate.get() == stream_info.prev_sample_rate.get()
            && stream_info.max_block_frames.get() == self.fixed_block.max_block_frames() as u32
        {
            return;
        }

        let settings = audionimbus::AudioSettings {
            sampling_rate: stream_info.sample_rate.get(),
            frame_size: self.fixed_block.frame_size() as u32,
        };

        self.ambisonics_encode_effect = audionimbus::AmbisonicsEncodeEffect::try_new(
            &STEAM_AUDIO_CONTEXT,
            &settings,
            &audionimbus::AmbisonicsEncodeEffectSettings {
                max_order: self.quality.order,
            },
        )
        .unwrap();
        self.direct_effect = audionimbus::DirectEffect::try_new(
            &STEAM_AUDIO_CONTEXT,
            &settings,
            &audionimbus::DirectEffectSettings { num_channels: 2 },
        )
        .unwrap();
        self.reflection_effect = audionimbus::ReflectionEffect::try_new(
            &STEAM_AUDIO_CONTEXT,
            &settings,
            &audionimbus::ReflectionEffectSettings::Convolution {
                impulse_response_size: impulse_response_size(self.quality, settings.sampling_rate),
                num_channels: self.num_channels(),
            },
        )
        .unwrap();
        self.pathing_effect = audionimbus::PathEffect::try_new(
            &STEAM_AUDIO_CONTEXT,
            &settings,
            &audionimbus::PathEffectSettings {
                max_order: self.quality.order,
                spatialization: Some(audionimbus::Spatialization {
                    speaker_layout: audionimbus::SpeakerLayout::Stereo,
                    hrtf: &self.hrtf,
                }),
            },
        )
        .unwrap();
        self.binaural_effect = audionimbus::BinauralEffect::try_new(
            &STEAM_AUDIO_CONTEXT,
            &settings,
            &audionimbus::BinauralEffectSettings { hrtf: &self.hrtf },
        )
        .unwrap();

        self.ambisonics_decode_effect = audionimbus::AmbisonicsDecodeEffect::try_new(
            &STEAM_AUDIO_CONTEXT,
            &settings,
            &audionimbus::AmbisonicsDecodeEffectSettings {
                max_order: self.quality.order,
                speaker_layout: audionimbus::SpeakerLayout::Stereo,
                hrtf: &self.hrtf,
            },
        )
        .unwrap();

        let fixed_block_size = self.fixed_block.inputs.channel_capacity;
        let max_output_size = stream_info.max_block_frames.get() as usize;
        self.fixed_block.resize(fixed_block_size, max_output_size);
    }
}

pub(crate) struct SimulationOutputEvent {
    pub(crate) flags: audionimbus::SimulationFlags,
    pub(crate) outputs: audionimbus::SimulationOutputs,
}

fn impulse_response_size(quality: SteamAudioQuality, sampling_rate: u32) -> u32 {
    (quality.reflections.impulse_duration.as_secs_f32() * sampling_rate as f32).ceil() as u32
}

fn apply_volume_ramp(start_volume: f32, end_volume: f32, buffer: &mut [&mut [f32]]) {
    for channel in buffer {
        let sample_num = channel.len();
        for (i, sample) in channel.iter_mut().enumerate() {
            let fraction = i as f32 / sample_num as f32;
            let volume = fraction * end_volume + (1.0 - fraction) * start_volume;

            *sample *= volume;
        }
    }
}
