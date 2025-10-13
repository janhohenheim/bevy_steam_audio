use crate::{
    STEAM_AUDIO_CONTEXT,
    nodes::FixedProcessBlock,
    prelude::*,
    settings::{SteamAudioQuality, order_to_num_channels},
    wrapper::{AudionimbusCoordinateSystem, ChannelPtrs},
};

use audionimbus::{AudioBuffer, Spatialization};
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
    pub source_position: Vec3,
    pub listener_position: AudionimbusCoordinateSystem,
    pub pathing_available: bool,
}

impl Default for SteamAudioNode {
    fn default() -> Self {
        Self {
            direct_gain: 1.0,
            reflection_gain: 0.5,
            pathing_gain: 0.0,

            // Set by the plugin
            source_position: Vec3::ZERO,
            listener_position: AudionimbusCoordinateSystem::default(),
            pathing_available: false,
        }
    }
}

#[derive(Diff, Patch, Debug, Clone, RealtimeClone, PartialEq, Component, Default, Reflect)]
#[reflect(Component)]
#[component(on_add = on_add_steam_audio_node_config)]
pub struct SteamAudioNodeConfig {
    pub(crate) order: u32,
    pub(crate) frame_size: u32,
}

fn on_add_steam_audio_node_config(mut world: DeferredWorld, ctx: HookContext) {
    let quality = *world.resource::<SteamAudioQuality>();
    let mut entity = world.entity_mut(ctx.entity);
    let mut config = entity.get_mut::<SteamAudioNodeConfig>().unwrap();
    config.order = quality.order;
    config.frame_size = quality.frame_size;
}

impl SteamAudioNodeConfig {
    #[inline]
    fn num_channels(&self) -> u32 {
        order_to_num_channels(self.order)
    }
}

impl AudioNode for SteamAudioNode {
    type Configuration = SteamAudioNodeConfig;

    fn info(&self, config: &Self::Configuration) -> AudioNodeInfo {
        AudioNodeInfo::new()
            .debug_name("Steam Audio node")
            // 1 -> ambisonic order
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
        SteamAudioProcessor {
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
                    max_order: config.order,
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
                    max_order: config.order,
                    speaker_layout: audionimbus::SpeakerLayout::Stereo,
                    hrtf: &hrtf,
                },
            )
            .unwrap(),
            fixed_block: FixedProcessBlock::new(
                config.frame_size as usize,
                cx.stream_info.max_block_frames.get() as usize,
                2,
                2,
            ),
            direct_effect_params: None,
            reflection_effect_params: None,
            pathing_effect_params: None,
            order: config.order,
            hrtf,
            ambisonics_ptrs: ChannelPtrs::new(config.num_channels() as usize),
            ambisonics_buffer: core::iter::repeat_n(
                0f32,
                (config.frame_size * config.num_channels()) as usize,
            )
            .collect(),
        }
    }
}

struct SteamAudioProcessor {
    order: u32,
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
        order_to_num_channels(self.order)
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
        if proc_info.prev_output_was_silent && proc_info.in_silence_mask.all_channels_silent(1) {
            return ProcessStatus::ClearAllOutputs;
        }

        let (
            Some(direct_effect_params),
            Some(reflection_effect_params),
            Some(pathing_effect_params),
        ) = (
            self.direct_effect_params.as_ref(),
            self.reflection_effect_params.as_ref(),
            self.pathing_effect_params.as_mut(),
        )
        else {
            // If this is encountered at any point other than just
            // after insertion into the graph, then something's gone
            // quite wrong. So, we'll clear the fixed buffers.
            self.fixed_block.clear();
            return ProcessStatus::ClearAllOutputs;
        };

        let [scratch_direct_left, scratch_direct_right, scratch_mono] =
            extra.scratch_buffers.channels_mut::<3>();
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

            assert!(scratch_mono.len() >= frame_size);
            let mut channel_ptrs = [scratch_mono.as_mut_ptr()];
            // SAFETY:
            // `channel_ptrs` points to `frame_size` floats, whose lifetime
            // will outlast `mono_sa_buffer`.
            let mut mono_sa_buffer = unsafe {
                AudioBuffer::<&mut [f32], _>::try_new_borrowed(
                    channel_ptrs.as_mut_slice(),
                    frame_size as u32,
                )
                .unwrap()
            };
            mono_sa_buffer.downmix(&STEAM_AUDIO_CONTEXT, &input_sa_buffer);

            assert!(scratch_direct_left.len() >= frame_size);
            assert!(scratch_direct_right.len() >= frame_size);
            let mut channel_ptrs = [
                scratch_direct_left.as_mut_ptr(),
                scratch_direct_right.as_mut_ptr(),
            ];

            // Direct Effect

            // SAFETY:
            //
            // `channel_ptrs` points to `frame_size` floats, whose lifetime
            // will outlast `direct_sa_buffer`.
            let direct_sa_buffer = unsafe {
                AudioBuffer::<&mut [f32], _>::try_new_borrowed(
                    channel_ptrs.as_mut_slice(),
                    frame_size as u32,
                )
                .unwrap()
            };

            let _effect_state = self.direct_effect.apply(
                &direct_effect_params.clone(),
                &input_sa_buffer,
                &direct_sa_buffer,
            );

            // Binaural Effect
            let direction = source_position - listener.origin;
            let direction = audionimbus::Direction::new(direction.x, direction.y, direction.z);
            let binaural_params = audionimbus::BinauralEffectParams {
                direction: direction,
                interpolation: audionimbus::HrtfInterpolation::Nearest,
                spatial_blend: 1.0,
                hrtf: &self.hrtf,
                peak_delays: None,
            };

            let _effect_state =
                self.binaural_effect
                    .apply(&binaural_params, &direct_sa_buffer, &output_sa_buffer);

            // Reflection effect
            let settings = audionimbus::AudioBufferSettings {
                num_channels: Some(order_to_num_channels(self.order)),
                ..default()
            };
            let ambisonics_sa_buffer = AudioBuffer::try_borrowed_with_data_and_settings(
                &mut self.ambisonics_buffer,
                &mut self.ambisonics_ptrs,
                settings,
            )
            .unwrap();

            let _effect_state = self.reflection_effect.apply(
                reflection_effect_params,
                &mono_sa_buffer,
                &ambisonics_sa_buffer,
            );

            // Decode ambisonics
            let ambisonics_decode_effect_params = audionimbus::AmbisonicsDecodeEffectParams {
                order: self.order,
                hrtf: &self.hrtf,
                orientation: listener.to_audionimbus(),
                binaural: true,
            };
            let _effect_state = self.ambisonics_decode_effect.apply(
                &ambisonics_decode_effect_params,
                &ambisonics_sa_buffer,
                &direct_sa_buffer,
            );

            output_sa_buffer.mix(&STEAM_AUDIO_CONTEXT, &direct_sa_buffer);

            // Pathing effect
            if self.params.pathing_available {
                pathing_effect_params.order = self.order;
                pathing_effect_params.listener = self.params.listener_position.to_audionimbus();
                pathing_effect_params.binaural = true;
                pathing_effect_params.hrtf = self.hrtf.clone();

                let _effect_state = self.pathing_effect.apply(
                    pathing_effect_params,
                    &mono_sa_buffer,
                    &direct_sa_buffer,
                );

                output_sa_buffer.mix(&STEAM_AUDIO_CONTEXT, &direct_sa_buffer);
            }
        });

        // check for silence when the input is silent
        if matches!(result, ProcessStatus::OutputsModified)
            && proc_info.in_silence_mask.all_channels_silent(1)
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
                max_order: self.order,
            },
        )
        .unwrap();
        self.direct_effect = audionimbus::DirectEffect::try_new(
            &STEAM_AUDIO_CONTEXT,
            &settings,
            &audionimbus::DirectEffectSettings { num_channels: 1 },
        )
        .unwrap();
        self.reflection_effect = audionimbus::ReflectionEffect::try_new(
            &STEAM_AUDIO_CONTEXT,
            &settings,
            &audionimbus::ReflectionEffectSettings::Convolution {
                impulse_response_size: 2 * settings.sampling_rate,
                num_channels: self.num_channels(),
            },
        )
        .unwrap();
        self.pathing_effect = audionimbus::PathEffect::try_new(
            &STEAM_AUDIO_CONTEXT,
            &settings,
            &audionimbus::PathEffectSettings {
                max_order: self.order,
                spatialization: None,
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
