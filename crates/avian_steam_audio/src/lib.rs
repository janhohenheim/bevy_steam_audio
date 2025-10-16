use std::{
    hash::Hasher,
    sync::{Arc, Weak},
};

use avian3d::{
    parry::shape::{Shape, SharedShape, TypedShape},
    prelude::*,
};
use bevy_app::prelude::*;
use bevy_derive::{Deref, DerefMut};
use bevy_ecs::{entity_disabling::Disabled, prelude::*};
use bevy_platform::collections::{HashMap, HashTable};
use bevy_steam_audio::{
    STEAM_AUDIO_CONTEXT, SteamAudioSystems,
    prelude::*,
    scene::{
        InSteamAudioMeshSpawnQueue, SteamAudioInstancedMesh, SteamAudioRootScene,
        SteamAudioStaticMesh,
    },
    wrapper::ToSteamAudioTransform as _,
};
use bevy_transform::prelude::*;
use hashbrown::DefaultHashBuilder;
use std::hash::{BuildHasher as _, Hash};
use trimesh_builder::ColliderTrimeshBuilder as _;

use crate::trimesh_builder::Trimesh;

mod trimesh_builder;

pub mod prelude {
    pub use crate::AvianSteamAudioScenePlugin;
}

pub struct AvianSteamAudioScenePlugin;

impl Plugin for AvianSteamAudioScenePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            PostUpdate,
            (
                // if we modified or removed a mesh, first despawn it on steam audio's side
                garbage_collect_meshes,
                // then if an entity holding a mesh has been touched (when happens in any mutation case *except* when removed!)
                // we add it to the spawn queue
                queue_steam_audio_mesh_processing,
                // Since we already despawned modified meshes, we know that anything that was modified is safe to spawn again.
                spawn_new_steam_audio_meshes,
            )
                .chain()
                .in_set(SteamAudioSystems::MeshLifecycle),
        );
        app.init_resource::<ShapeToScene>();
    }
}

#[derive(Resource, Default, Deref, DerefMut)]
struct ShapeToScene(HashMap<ColliderKey, audionimbus::Scene>);

#[derive(Deref, DerefMut)]
struct ColliderKey(Weak<dyn Shape>);

impl From<&Collider> for ColliderKey {
    fn from(collider: &Collider) -> Self {
        ColliderKey(Arc::downgrade(&collider.shape_scaled().0))
    }
}

impl Eq for ColliderKey {}

impl PartialEq for ColliderKey {
    fn eq(&self, other: &Self) -> bool {
        Weak::ptr_eq(&self.0, &other.0)
    }
}
impl Hash for ColliderKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        Weak::as_ptr(&self.0).hash(state);
    }
}

fn queue_steam_audio_mesh_processing(
    colliders: Query<(Entity, Ref<Collider>), With<SteamAudioMaterial>>,
    mut commands: Commands,
) {
    for (entity, mesh) in colliders.iter() {
        if mesh.is_changed() {
            commands.entity(entity).insert(InSteamAudioMeshSpawnQueue);
        }
    }
}

fn spawn_new_steam_audio_meshes(
    mut commands: Commands,
    mut map: ResMut<ShapeToScene>,
    queued: Query<
        (
            Entity,
            NameOrEntity,
            &Collider,
            &SteamAudioMaterial,
            &GlobalTransform,
            Has<Static>,
        ),
        (Allow<Disabled>, With<InSteamAudioMeshSpawnQueue>),
    >,
    mut root: ResMut<SteamAudioRootScene>,
    mut errors: Local<Vec<String>>,
) -> Result {
    errors.clear();
    for (entity, name, collider, material, transform, is_static) in &queued {
        if !is_static {
            let sub_scene = if let Some(sub_scene) = map.get(&ColliderKey::from(collider)) {
                sub_scene.clone()
            } else {
                let mut sub_scene = match audionimbus::Scene::try_new(
                    &STEAM_AUDIO_CONTEXT,
                    &audionimbus::SceneSettings::default(),
                ) {
                    Ok(sub_scene) => sub_scene,
                    Err(err) => {
                        errors.push(format!(
                            "{name}: Failed to create sub-scene for mesh: {err}"
                        ));
                        commands
                            .entity(entity)
                            .try_remove::<InSteamAudioMeshSpawnQueue>();
                        continue;
                    }
                };
                let mesh = match collider.trimesh_builder().build() {
                    Ok(mesh) => mesh,
                    Err(err) => {
                        errors.push(format!("{name}: Failed to build mesh: {err}"));
                        commands
                            .entity(entity)
                            .try_remove::<InSteamAudioMeshSpawnQueue>();
                        continue;
                    }
                };
                let static_mesh = match mesh.to_steam_audio_mesh(&sub_scene, (*material).into()) {
                    Ok(mesh) => mesh,
                    Err(err) => {
                        errors.push(format!("{name}: Failed to convert mesh: {err}"));
                        commands
                            .entity(entity)
                            .try_remove::<InSteamAudioMeshSpawnQueue>();
                        continue;
                    }
                };
                sub_scene.add_static_mesh(static_mesh);
                // committing a new scene should be fine during simulation of a different scene
                sub_scene.commit();
                map.insert(collider.into(), sub_scene.clone());
                sub_scene
            };
            let transform = transform.to_steam_audio_transform();

            let instanced_mesh_settings = audionimbus::InstancedMeshSettings {
                sub_scene: sub_scene.clone(),
                transform,
            };
            let instanced_mesh =
                match audionimbus::InstancedMesh::try_new(&root, instanced_mesh_settings) {
                    Ok(instanced_mesh) => instanced_mesh,
                    Err(err) => {
                        errors.push(format!("{name}: Failed to create instanced mesh: {err}"));
                        commands
                            .entity(entity)
                            .try_remove::<InSteamAudioMeshSpawnQueue>();
                        continue;
                    }
                };
            root.add_instanced_mesh(instanced_mesh.clone());
            commands
                .entity(entity)
                .try_insert(SteamAudioInstancedMesh(instanced_mesh));
        } else {
            let mesh = match collider
                .trimesh_builder()
                .translated(transform.translation())
                .rotated(transform.rotation())
                .build()
            {
                Ok(mesh) => mesh,
                Err(err) => {
                    errors.push(format!("{name}: Failed to build mesh: {err}"));
                    commands
                        .entity(entity)
                        .try_remove::<InSteamAudioMeshSpawnQueue>();
                    continue;
                }
            };
            let static_mesh = match mesh.to_steam_audio_mesh(&root, (*material).into()) {
                Ok(mesh) => mesh,
                Err(err) => {
                    errors.push(format!("{name}: Failed to convert mesh: {err}"));
                    commands
                        .entity(entity)
                        .try_remove::<InSteamAudioMeshSpawnQueue>();
                    continue;
                }
            };
            root.add_static_mesh(static_mesh.clone());
            commands
                .entity(entity)
                .try_insert(SteamAudioStaticMesh(static_mesh));
        }

        commands
            .entity(entity)
            .try_remove::<InSteamAudioMeshSpawnQueue>();
        continue;
    }
    // Do not call root.commit(), it's not safe while simulations are running

    if !errors.is_empty() {
        Err(errors.join("\n").into())
    } else {
        Ok(())
    }
}

fn garbage_collect_meshes(mut map: ResMut<ShapeToScene>) {
    map.retain(|shape, _| Weak::strong_count(&shape) > 0);
}

trait ToSteamAudioMesh {
    fn to_steam_audio_mesh(
        &self,
        scene: &audionimbus::Scene,
        material: audionimbus::Material,
    ) -> Result<audionimbus::StaticMesh>;
}

impl ToSteamAudioMesh for Trimesh {
    fn to_steam_audio_mesh(
        &self,
        scene: &audionimbus::Scene,
        material: audionimbus::Material,
    ) -> Result<audionimbus::StaticMesh> {
        let vertices = self
            .vertices
            .iter()
            .map(|v| audionimbus::Vector3::from(v.to_array()))
            .collect::<Vec<_>>();
        let triangles = self
            .indices
            .iter()
            .map(|[v0, v1, v2]| audionimbus::Triangle::new(*v0 as i32, *v1 as i32, *v2 as i32))
            .collect::<Vec<_>>();
        let settings = audionimbus::StaticMeshSettings {
            vertices: &vertices,
            triangles: &triangles,
            material_indices: &vec![0; triangles.len()],
            materials: &[material],
        };
        audionimbus::StaticMesh::try_new(scene, &settings).map_err(Into::into)
    }
}
