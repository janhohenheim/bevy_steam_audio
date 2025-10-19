use audionimbus::AudioBuffer;
use bevy_ecs::{lifecycle::HookContext, world::DeferredWorld};
use bevy_seedling::{node::RegisterNode as _, prelude::*};
use firewheel::{
    channel_config::ChannelConfig,
    diff::{Diff, Patch, RealtimeClone},
    event::ProcEvents,
    node::{
        AudioNode, AudioNodeInfo, AudioNodeProcessor, ConstructProcessorContext, ProcBuffers,
        ProcExtra, ProcInfo, ProcessStatus,
    },
};

use crate::{
    nodes::{FixedProcessBlock, apply_volume_ramp},
    prelude::*,
    wrapper::{AudionimbusCoordinateSystem, ChannelPtrs},
};

pub(super) fn plugin(app: &mut App) {
    app.register_node::<SteamAudioReverbNode>();
}

#[derive(Diff, Patch, Debug, PartialEq, Clone, RealtimeClone, Component, Reflect)]
#[reflect(Component)]
pub struct SteamAudioReverbNode {
    pub gain: f32,
    pub previous_gain: f32,
    pub listener_position: AudionimbusCoordinateSystem,
}

impl Default for SteamAudioReverbNode {
    fn default() -> Self {
        Self {
            gain: 1.0,
            previous_gain: 1.0,
            listener_position: default(),
        }
    }
}

#[derive(Debug, Clone, RealtimeClone, PartialEq, Component, Default)]
#[component(on_add = on_add_steam_audio_reverb_node_config)]
pub struct SteamAudioReverbNodeConfig {
    pub(crate) hrtf: Option<audionimbus::Hrtf>,
    pub(crate) quality: SteamAudioQuality,
}

fn on_add_steam_audio_reverb_node_config(mut world: DeferredWorld, ctx: HookContext) {
    let quality = *world.resource::<SteamAudioQuality>();
    let mut entity = world.entity_mut(ctx.entity);
    let mut config = entity.get_mut::<SteamAudioReverbNodeConfig>().unwrap();
    config.quality = quality;
}

impl AudioNode for SteamAudioReverbNode {
    type Configuration = SteamAudioReverbNodeConfig;

    fn info(&self, _: &Self::Configuration) -> AudioNodeInfo {
        AudioNodeInfo::new()
            .debug_name("Steam Audio reverb node")
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
        let hrtf = config.hrtf.clone().expect("Created an `AudioNode` before the audio stream was ready. Please wait until `SteamAudioReady` is triggered.");
        let settings = audionimbus::AudioSettings {
            sampling_rate: cx.stream_info.sample_rate.into(),
            frame_size: config.quality.frame_size,
        };
        SteamAudioReverbNodeProcessor {
            source: None,
            reflection_effect: audionimbus::ReflectionEffect::try_new(
                &STEAM_AUDIO_CONTEXT,
                &settings,
                &audionimbus::ReflectionEffectSettings::Convolution {
                    impulse_response_size: config
                        .quality
                        .impulse_response_size(cx.stream_info.sample_rate.into()),
                    num_channels: config.quality.num_channels(),
                },
            )
            .unwrap(),
            params: self.clone(),
            quality: config.quality,
            hrtf: hrtf.clone(),
            ambisonics_ptrs: ChannelPtrs::new(config.quality.num_channels() as usize),
            ambisonics_buffer: core::iter::repeat_n(
                0f32,
                (config.quality.frame_size * config.quality.num_channels()) as usize,
            )
            .collect(),
            fixed_block: FixedProcessBlock::new(
                config.quality.frame_size as usize,
                cx.stream_info.max_block_frames.get() as usize,
                2,
                2,
            ),
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
        }
    }
}

struct SteamAudioReverbNodeProcessor {
    quality: SteamAudioQuality,
    hrtf: audionimbus::Hrtf,
    params: SteamAudioReverbNode,
    source: Option<audionimbus::Source>,
    reflection_effect: audionimbus::ReflectionEffect,
    ambisonics_decode_effect: audionimbus::AmbisonicsDecodeEffect,
    // We might be able to use the scratch buffers for this, but
    // the ambisonic order may produce more channels than scratch
    // buffers.
    ambisonics_buffer: Box<[f32]>,
    ambisonics_ptrs: ChannelPtrs,
    fixed_block: FixedProcessBlock,
}

impl AudioNodeProcessor for SteamAudioReverbNodeProcessor {
    fn process(
        &mut self,
        proc_info: &ProcInfo,
        proc_buffers: ProcBuffers,
        events: &mut ProcEvents,
        extra: &mut ProcExtra,
    ) -> ProcessStatus {
        for mut event in events.drain() {
            if let Some(patch) = SteamAudioReverbNode::patch_event(&event) {
                Patch::apply(&mut self.params, patch);
            }
            if let Some(source) = event.downcast::<audionimbus::Source>() {
                self.source = Some(source);
            }
        }

        // If the previous output of this node was silent, and the inputs are also silent
        // then we know there is no reverb tail left and we can skip processing.
        if proc_info.prev_output_was_silent && proc_info.in_silence_mask.all_channels_silent(2) {
            return ProcessStatus::ClearAllOutputs;
        }

        let Some(mut source) = self.source.clone() else {
            // If this is encountered at any point other than just
            // after insertion into the graph, then something's gone
            // quite wrong. So, we'll clear the fixed buffers.
            self.fixed_block.clear();
            return ProcessStatus::ClearAllOutputs;
        };

        let mut reflection_effect_params = source
            .get_outputs(audionimbus::SimulationFlags::REFLECTIONS)
            .reflections()
            .into_inner();

        let scratch_mono = extra.scratch_buffers.first_mut();
        let frame_size = self.fixed_block.frame_size();
        let fixed_block = &mut self.fixed_block;
        let temp_proc = ProcBuffers {
            inputs: proc_buffers.inputs,
            outputs: proc_buffers.outputs,
        };
        let result = fixed_block.process(temp_proc, proc_info, |inputs, outputs| {
            let listener = self.params.listener_position;

            assert_eq!(inputs[0].len(), frame_size);
            let mut channel_ptrs = [inputs[0].as_ptr().cast_mut(), inputs[1].as_ptr().cast_mut()];

            // SAFETY:
            // `channel_ptrs` points to `frame_size` floats, whose lifetime
            // will outlast `input_sa_buffer`.
            let input_sa_buffer = unsafe {
                AudioBuffer::<&[f32], _>::try_new(channel_ptrs.as_mut_slice(), frame_size as u32)
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
            let output_sa_buffer = unsafe {
                AudioBuffer::<&mut [f32], _>::try_new(
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
                AudioBuffer::<&mut [f32], _>::try_new(
                    channel_ptrs.as_mut_slice(),
                    frame_size as u32,
                )
                .unwrap()
            };
            mono_sa_buffer.downmix(&STEAM_AUDIO_CONTEXT, &input_sa_buffer);

            reflection_effect_params.reflection_effect_type = self.quality.reflections.kind.into();
            reflection_effect_params.num_channels = self.quality.num_channels();
            reflection_effect_params.impulse_response_size = self
                .quality
                .impulse_response_size(proc_info.sample_rate.into());

            apply_volume_ramp(
                self.params.previous_gain,
                self.params.gain,
                &mut [scratch_mono],
            );
            self.params.previous_gain = self.params.gain;

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

            let _effect_state = self.reflection_effect.apply(
                &reflection_effect_params,
                &mono_sa_buffer,
                &ambisonics_sa_buffer,
            );

            let decode_params = audionimbus::AmbisonicsDecodeEffectParams {
                order: self.quality.order,
                hrtf: &self.hrtf,
                orientation: listener.into(),
                binaural: true,
            };
            let _effect_state = self.ambisonics_decode_effect.apply(
                &decode_params,
                &ambisonics_sa_buffer,
                &output_sa_buffer,
            );
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
}
