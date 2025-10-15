use bevy_app::prelude::*;

pub mod prelude {
    pub use crate::AvianSteamAudioScenePlugin;
}

pub struct AvianSteamAudioScenePlugin;

impl Plugin for AvianSteamAudioScenePlugin {
    fn build(&self, _app: &mut App) {}
}
