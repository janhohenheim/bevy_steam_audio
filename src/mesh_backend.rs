use std::{iter, marker::PhantomData};

use crate::{
    backend::{SceneSettings, SteamAudioApp as _, TriMesh},
    prelude::*,
};

pub struct Mesh3dBackendPlugin {
    _pd: PhantomData<()>,
}

impl Default for Mesh3dBackendPlugin {
    fn default() -> Self {
        Self { _pd: PhantomData }
    }
}

impl Plugin for Mesh3dBackendPlugin {
    fn build(&self, app: &mut App) {
        app.set_steam_audio_scene_backend(build_scene);
    }
}

#[derive(Component, Debug, Clone, Copy, Deref, DerefMut, PartialEq, Reflect)]
#[reflect(Component)]
pub struct MeshSteamAudioMaterial(pub SteamAudioMaterial);

fn build_scene(
    In(settings): In<SceneSettings>,
    meshes: Res<Assets<Mesh>>,
    mesh_handles: Query<(
        Entity,
        &GlobalTransform,
        &Mesh3d,
        Option<&MeshSteamAudioMaterial>,
    )>,
) -> TriMesh {
    let mut materials = Vec::new();
    let mut material_indices = Vec::new();
    let mut trimesh = mesh_handles
        .iter()
        .filter_map(|(entity, transform, mesh, material)| {
            if settings
                .filter
                .as_ref()
                .is_some_and(|entities| !entities.contains(&entity))
            {
                return None;
            }
            let transform = transform.compute_transform();
            let mesh = meshes.get(mesh)?.clone().transformed_by(transform);
            let trimesh = TriMesh::from_mesh(&mesh)?;

            if let Some(material) = material {
                let index = if let Some(index) = materials.iter().position(|m| *m == material.0) {
                    index
                } else {
                    materials.push(material.0);
                    materials.len() - 1
                };
                material_indices
                    .extend(iter::repeat_n(index as u32, trimesh.material_indices.len()));
            }

            Some(trimesh)
        })
        .fold(TriMesh::default(), |mut acc, t| {
            acc.extend(t);
            acc
        });
    trimesh.materials = materials;
    trimesh.material_indices = material_indices;
    trimesh
}
