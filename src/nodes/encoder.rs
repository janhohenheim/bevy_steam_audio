use crate::{
    STEAM_AUDIO_CONTEXT,
    nodes::{FixedProcessBlock, reverb::SharedReverbData},
    prelude::*,
    settings::{SteamAudioQuality, order_to_num_channels},
    wrapper::ChannelPtrs,
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
    pub reverb_gain: f32,
    pub source_position: Vec3,
    pub listener_position: Vec3,
}

impl Default for SteamAudioNode {
    fn default() -> Self {
        Self {
            direct_gain: 1.0,
            reflection_gain: 0.5,
            reverb_gain: 0.1,
            source_position: Vec3::ZERO,
            listener_position: Vec3::ZERO,
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
            fixed_block: FixedProcessBlock::new(
                config.frame_size as usize,
                cx.stream_info.max_block_frames.get() as usize,
                1,
                config.num_channels() as usize,
            ),
            direct_effect_params: None,
            reflection_effect_params: None,
            order: config.order,
            ambisonics_ptrs: vec![std::ptr::null_mut(); config.num_channels() as usize].into(),
            ambisonics_buffer: vec![0.0; (config.frame_size * config.num_channels()) as usize],
        }
    }
}

struct SteamAudioProcessor {
    order: u32,
    params: SteamAudioNode,
    ambisonics_encode_effect: audionimbus::AmbisonicsEncodeEffect,
    direct_effect: audionimbus::DirectEffect,
    reflection_effect: audionimbus::ReflectionEffect,
    reverb_effect: audionimbus::ReflectionEffect,
    fixed_block: FixedProcessBlock,
    direct_effect_params: Option<audionimbus::DirectEffectParams>,
    reflection_effect_params: Option<audionimbus::ReflectionEffectParams>,
    // We might be able to use the scratch buffers for this, but
    // the ambisonic order may produce more channels than scratch
    // buffers.
    ambisonics_buffer: Vec<f32>,
    ambisonics_ptrs: ChannelPtrs,
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
            }
        }

        // Don't early return on silent inputs: there is probably reverb left
        // TODO: actually check for this silence like freeverb

        let (
            Some(direct_effect_params),
            Some(reflection_effect_params),
            Some(SharedReverbData(reverb_effect_params)),
        ) = (
            self.direct_effect_params.as_ref(),
            self.reflection_effect_params.as_ref(),
            extra.store.try_get::<SharedReverbData>(),
        )
        else {
            // If this is encountered at any point other than just
            // after insertion into the graph, then something's gone
            // quite wrong. So, we'll clear the fixed buffers.
            self.fixed_block.clear();
            return ProcessStatus::ClearAllOutputs;
        };

        let scratch_direct = extra.scratch_buffers.first_mut();
        let frame_size = self.fixed_block.frame_size();

        let fixed_block = &mut self.fixed_block;
        fixed_block.process(proc_buffers, proc_info, |inputs, outputs| {
            let source_position = self.params.source_position;

            assert_eq!(inputs[0].len(), frame_size);
            let mut channel_ptrs = [inputs[0].as_ptr() as *mut _];

            // # Safety
            //
            // `channel_ptrs` points to `frame_size` floats, whose lifetime
            // will outlast `input_sa_buffer`.
            let input_sa_buffer = unsafe {
                AudioBuffer::<&[f32], _>::try_new_borrowed(
                    channel_ptrs.as_mut_slice(),
                    frame_size as u32,
                )
                .unwrap()
            };

            assert!(scratch_direct.len() >= frame_size);
            let mut channel_ptrs = [scratch_direct.as_mut_ptr()];

            // # Safety
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

            let listener_position = self.params.listener_position;
            let direction = source_position - listener_position;
            let direction = audionimbus::Direction::new(direction.x, direction.y, direction.z);

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

            let ambisonics_encode_effect_params = audionimbus::AmbisonicsEncodeEffectParams {
                direction,
                order: self.order,
            };
            let _effect_state = self.ambisonics_encode_effect.apply(
                &ambisonics_encode_effect_params,
                &direct_sa_buffer,
                &ambisonics_sa_buffer,
            );

            accumulate_in_output(&ambisonics_sa_buffer, outputs, self.params.direct_gain);

            let _effect_state = self.reflection_effect.apply(
                reflection_effect_params,
                &input_sa_buffer,
                &ambisonics_sa_buffer,
            );

            accumulate_in_output(&ambisonics_sa_buffer, outputs, self.params.reflection_gain);

            let _effect_state = self.reverb_effect.apply(
                reverb_effect_params,
                &input_sa_buffer,
                &ambisonics_sa_buffer,
            );

            accumulate_in_output(&ambisonics_sa_buffer, outputs, self.params.reverb_gain);
        })
    }

    fn new_stream(
        &mut self,
        stream_info: &firewheel::StreamInfo,
        _context: &mut firewheel::node::ProcStreamCtx,
    ) {
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
        self.reverb_effect = audionimbus::ReflectionEffect::try_new(
            &STEAM_AUDIO_CONTEXT,
            &settings,
            &audionimbus::ReflectionEffectSettings::Convolution {
                impulse_response_size: 2 * settings.sampling_rate,
                num_channels: self.num_channels(),
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

/// Accumulate a steam audio buffer into the output.
fn accumulate_in_output(
    sa_buffer: &AudioBuffer<&mut Vec<f32>, &mut [*mut f32]>,
    outputs: &mut [&mut [f32]],
    gain: f32,
) {
    for (i, channel) in sa_buffer.channels().enumerate() {
        for (frame, sample) in channel.iter().enumerate() {
            outputs[i][frame] += sample * gain;
        }
    }
}
