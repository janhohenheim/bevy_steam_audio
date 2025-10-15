use crate::{
    STEAM_AUDIO_CONTEXT,
    nodes::FixedProcessBlock,
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
    app.register_node::<AmbisonicDecodeNode>();
}

#[derive(Diff, Patch, Debug, Default, PartialEq, Clone, RealtimeClone, Component, Reflect)]
#[reflect(Component)]
pub struct AmbisonicDecodeNode {
    pub(crate) listener_orientation: AudionimbusCoordinateSystem,
}

#[derive(Diff, Patch, Debug, Clone, RealtimeClone, PartialEq, Default, Component, Reflect)]
#[reflect(Component)]
#[component(on_add = on_add_decode_node_config)]
pub struct SteamAudioDecodeNodeConfig {
    /// Set to `None` to use the global [`SteamAudioQuality::order`].
    /// Set to `Some` if this is for some custom ambisonic audio you want to decode.
    pub order: Option<u32>,
    pub frame_size: u32,
}

fn on_add_decode_node_config(mut world: DeferredWorld, ctx: HookContext) {
    let quality = *world.resource::<SteamAudioQuality>();
    let mut entity = world.entity_mut(ctx.entity);
    let mut config = entity.get_mut::<SteamAudioDecodeNodeConfig>().unwrap();
    if config.order.is_none() {
        config.order = Some(quality.order);
    }
    config.frame_size = quality.frame_size;
}

impl SteamAudioDecodeNodeConfig {
    pub(crate) fn num_channels(&self) -> u32 {
        order_to_num_channels(self.order.unwrap())
    }
}

impl AudioNode for AmbisonicDecodeNode {
    type Configuration = SteamAudioDecodeNodeConfig;

    fn info(&self, config: &Self::Configuration) -> AudioNodeInfo {
        AudioNodeInfo::new()
            .debug_name("Ambisonic decode node")
            .channel_config(ChannelConfig {
                num_inputs: ChannelCount::new(config.num_channels()).unwrap(),
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
            fixed_block: FixedProcessBlock::new(
                config.frame_size as usize,
                cx.stream_info.max_block_frames.get() as usize,
                config.num_channels() as usize,
                2,
            ),
            params: self.clone(),
            hrtf: hrtf.clone(),
            ambisonics_decode_effect: audionimbus::AmbisonicsDecodeEffect::try_new(
                &STEAM_AUDIO_CONTEXT,
                &settings,
                &audionimbus::AmbisonicsDecodeEffectSettings {
                    max_order: config.order.unwrap(),
                    speaker_layout: audionimbus::SpeakerLayout::Stereo,
                    hrtf: &hrtf,
                },
            )
            .unwrap(),
            order: config.order.unwrap(),
            frame_size: config.frame_size,
            mix_ptrs: ChannelPtrs::new(config.num_channels() as usize),
        }
    }
}

struct SteamAudioDecodeProcessor {
    fixed_block: FixedProcessBlock,
    params: AmbisonicDecodeNode,
    hrtf: audionimbus::Hrtf,
    ambisonics_decode_effect: audionimbus::AmbisonicsDecodeEffect,
    order: u32,
    frame_size: u32,
    mix_ptrs: ChannelPtrs,
}

impl SteamAudioDecodeProcessor {
    #[inline]
    fn num_channels(&self) -> u32 {
        order_to_num_channels(self.order)
    }
}

impl AudioNodeProcessor for SteamAudioDecodeProcessor {
    fn process(
        &mut self,
        proc_info: &ProcInfo,
        proc_buffers: ProcBuffers,
        events: &mut ProcEvents,
        _: &mut ProcExtra,
    ) -> ProcessStatus {
        for patch in events.drain_patches::<AmbisonicDecodeNode>() {
            Patch::apply(&mut self.params, patch);
        }

        if proc_info
            .in_silence_mask
            .all_channels_silent(proc_buffers.inputs.len())
            && self.fixed_block.inputs_clear()
        {
            return ProcessStatus::ClearAllOutputs;
        }

        let channels = self.num_channels();
        let fixed_block = &mut self.fixed_block;
        fixed_block.process(proc_buffers, proc_info, |inputs, outputs| {
            #[expect(clippy::needless_range_loop, reason = "More readable like this")]
            for channel in 0..channels as usize {
                let channel_buffer = &inputs[channel];
                assert_eq!(channel_buffer.len(), self.frame_size as usize);
                self.mix_ptrs[channel] = channel_buffer.as_ptr().cast_mut();
            }

            // SAFETY:
            //
            // The inputs pointers refer to valid memory with the
            // correct length. While we've passed around *mut pointers,
            // they will never be written to.
            let input_sa_buffer = unsafe {
                audionimbus::AudioBuffer::<&[f32], _>::try_new(
                    self.mix_ptrs.as_mut(),
                    self.frame_size,
                )
                .unwrap()
            };

            let (left, right) = outputs.split_at_mut(1);

            assert_eq!(left[0].len(), self.frame_size as usize);
            assert_eq!(right[0].len(), self.frame_size as usize);

            let mut channel_ptrs = [left[0].as_mut_ptr(), right[0].as_mut_ptr()];

            // SAFETY:
            //
            // The inputs pointers refer to valid, non-aliased memory with the
            // correct length.
            let output_sa_buffer = unsafe {
                audionimbus::AudioBuffer::<&mut [f32], _>::try_new(
                    channel_ptrs.as_mut_slice(),
                    self.frame_size,
                )
                .unwrap()
            };

            let ambisonics_decode_effect_params = audionimbus::AmbisonicsDecodeEffectParams {
                order: self.order,
                hrtf: &self.hrtf,
                orientation: self.params.listener_orientation.to_audionimbus(),
                binaural: true,
            };
            let _effect_state = self.ambisonics_decode_effect.apply(
                &ambisonics_decode_effect_params,
                &input_sa_buffer,
                &output_sa_buffer,
            );
        })
    }

    fn new_stream(
        &mut self,
        stream_info: &firewheel::StreamInfo,
        _context: &mut firewheel::node::ProcStreamCtx,
    ) {
        if stream_info.sample_rate.get() == stream_info.prev_sample_rate.get()
            && stream_info.max_block_frames.get() == self.fixed_block.max_block_frames() as u32
        {
            return;
        }

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

        let fixed_block_size = self.fixed_block.inputs.channel_capacity;
        let max_output_size = stream_info.max_block_frames.get() as usize;
        self.fixed_block.resize(fixed_block_size, max_output_size);
    }
}
