use bevy_app::prelude::*;

mod trimesh_builder;
use trimesh_builder::ColliderTrimeshBuilder as _;

pub mod prelude {
    pub use crate::AvianSteamAudioScenePlugin;
}

pub struct AvianSteamAudioScenePlugin;

impl Plugin for AvianSteamAudioScenePlugin {
    fn build(&self, _app: &mut App) {}
}
