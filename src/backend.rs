use bevy_ecs::system::SystemId;
use bevy_platform::collections::HashSet;

use crate::{material::SteamAudioMaterial, prelude::*};

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
#[derive(Debug, Clone, PartialEq, Reflect)]
pub struct SceneSettings {
    pub filter: Option<HashSet<Entity>>,
}

impl Default for SceneSettings {
    fn default() -> Self {
        Self { filter: None }
    }
}

#[derive(Debug, Reflect, Clone, PartialEq, Default)]
pub struct TriMesh {
    pub vertices: Vec<Vec3A>,
    pub indices: Vec<UVec3>,
    pub material_indices: Vec<u32>,
    pub materials: Vec<SteamAudioMaterial>,
}

impl TriMesh {
    /// Extends the trimesh with the vertices and indices of another trimesh.
    /// The indices of `other` will be offset by the number of vertices in `self`.
    pub fn extend(&mut self, other: TriMesh) {
        if self.vertices.len() > u32::MAX as usize {
            panic!("Cannot extend a trimesh with more than 2^32 vertices");
        }
        let next_vertex_index = self.vertices.len() as u32;
        self.vertices.extend(other.vertices);
        self.indices
            .extend(other.indices.iter().map(|i| i + next_vertex_index));
        let next_material_index = self.material_indices.len() as u32;
        self.material_indices.extend(
            other
                .material_indices
                .iter()
                .map(|i| i + next_material_index),
        );
        self.materials.extend(other.materials);
    }
}
