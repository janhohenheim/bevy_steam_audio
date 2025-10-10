use crate::{STEAM_AUDIO_CONTEXT, prelude::*};

mod backend;
pub mod mesh_backend;
mod trimesh;

pub use backend::*;
pub use trimesh::*;

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<SteamAudioRootScene>();
    app.add_plugins((backend::plugin, mesh_backend::plugin, trimesh::plugin));
}

#[derive(Resource, Deref, DerefMut)]
pub struct SteamAudioRootScene(audionimbus::Scene);

impl Default for SteamAudioRootScene {
    fn default() -> Self {
        Self(
            audionimbus::Scene::try_new(
                &STEAM_AUDIO_CONTEXT,
                &audionimbus::SceneSettings::default(),
            )
            .unwrap(),
        )
    }
}
