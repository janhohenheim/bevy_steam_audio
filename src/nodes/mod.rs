use crate::{nodes::reverb::ReverbDataNode, prelude::*, settings::SteamAudioQuality};
use bevy_seedling::prelude::*;
pub(crate) mod decoder;
pub(crate) mod encoder;
pub(crate) mod reverb;

pub use decoder::*;
pub use encoder::*;

pub(super) fn plugin(app: &mut App) {
    app.add_systems(PreStartup, setup_nodes);
    app.add_plugins((decoder::plugin, encoder::plugin, reverb::plugin));
    app.register_required_components::<SteamAudioPool, Transform>()
        .register_required_components::<SteamAudioPool, GlobalTransform>();
}

#[derive(PoolLabel, PartialEq, Eq, Debug, Hash, Clone, Default)]
pub struct SteamAudioPool;

#[derive(NodeLabel, PartialEq, Eq, Debug, Hash, Clone)]
pub struct SteamAudioDecodeBus;

pub(crate) fn setup_nodes(mut commands: Commands, quality: Res<SteamAudioQuality>) {
    // we only need one decoder
    commands.spawn((SteamAudioDecodeBus, SteamAudioDecodeNode::default()));
    commands.spawn(ReverbDataNode);

    // Copy-paste this part if you want to set up your own pool!
    commands
        .spawn((
            SamplerPool(SteamAudioPool),
            VolumeNodeConfig {
                channels: NonZeroChannelCount::new(quality.num_channels()).unwrap(),
            },
            sample_effects![SteamAudioNode::default()],
        ))
        .connect(SteamAudioDecodeBus);
}
