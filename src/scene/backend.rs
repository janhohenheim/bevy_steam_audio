use bevy_ecs::system::SystemId;
use bevy_platform::collections::HashSet;

use crate::{STEAM_AUDIO_CONTEXT, prelude::*, scene::TriMesh};

pub(super) fn plugin(app: &mut App) {
    let _ = app;
}

struct SteamAudioMesh(Handle<Mesh>);

/// The current backend registered through [`NavmeshApp::set_steam_audio_scene_backend`]
#[derive(Resource, Debug, Clone, Deref, DerefMut)]
pub struct SteamAudioSceneBackend(pub SystemId<In<SceneSettings>, TriMesh>);

/// Extension used to implement [`SteamAudioApp::set_steam_audio_scene_backend`] on [`App`]
pub trait SteamAudioApp {
    fn set_steam_audio_scene_backend<M>(
        &mut self,
        system: impl IntoSystem<In<SceneSettings>, TriMesh, M> + 'static,
    ) -> &mut App;
}

impl SteamAudioApp for App {
    fn set_steam_audio_scene_backend<M>(
        &mut self,
        system: impl IntoSystem<In<SceneSettings>, TriMesh, M> + 'static,
    ) -> &mut App {
        let id = self.register_system(system);
        self.world_mut().insert_resource(SteamAudioSceneBackend(id));
        self
    }
}

/// The input passed to the navmesh backend system.
#[derive(Debug, Clone, PartialEq, Reflect, Default)]
pub struct SceneSettings {
    pub filter: Option<HashSet<Entity>>,
}
