use crate::{STEAM_AUDIO_CONTEXT, prelude::*};

pub mod mesh_backend;

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<SteamAudioRootScene>();
}

#[derive(Resource, Deref, DerefMut)]
pub struct SteamAudioRootScene(pub audionimbus::Scene);

impl Default for SteamAudioRootScene {
    fn default() -> Self {
        let mut scene = audionimbus::Scene::try_new(
            &STEAM_AUDIO_CONTEXT,
            &audionimbus::SceneSettings::default(),
        )
        .unwrap();
        scene.commit();
        Self(scene)
    }
}
