use std::{
    hash::Hasher,
    sync::{Arc, Weak},
};

use avian3d::{parry::shape::Shape, prelude::*};
use bevy_app::prelude::*;
use bevy_derive::{Deref, DerefMut};
use bevy_ecs::{entity_disabling::Disabled, prelude::*};
use bevy_platform::collections::HashMap;
use bevy_reflect::prelude::*;
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
use std::hash::Hash;
use trimesh_builder::ColliderTrimeshBuilder as _;

use crate::trimesh_builder::Trimesh;

mod trimesh_builder;

pub mod prelude {
    pub use crate::AvianSteamAudioScenePlugin;
}

pub struct AvianSteamAudioScenePlugin;

#[derive(Resource, Debug, Eq, PartialEq, Reflect)]
#[reflect(Resource)]
pub struct AvianSteamAudioSettings {
    pub auto_insert_materials: bool,
}

impl Default for AvianSteamAudioSettings {
    fn default() -> Self {
        Self {
            auto_insert_materials: true,
        }
    }
}

impl Plugin for AvianSteamAudioScenePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<AvianSteamAudioSettings>();
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
        app.add_observer(add_collider)
            .add_observer(remove_collider_of)
            .add_observer(add_sensor);
        app.init_resource::<ShapeToScene>();
    }
}

fn remove_collider_of(remove: On<Remove, ColliderOf>, mut commands: Commands) {
    commands
        .entity(remove.entity)
        .try_remove::<SteamAudioMaterial>();
}

fn add_sensor(add: On<Add, Sensor>, mut commands: Commands) {
    commands
        .entity(add.entity)
        .try_remove::<SteamAudioMaterial>();
}

fn add_collider(
    add: On<Add, ColliderOf>,
    collider: Query<(Has<Sensor>, Has<SteamAudioMaterial>), Allow<Disabled>>,
    mut commands: Commands,
    rigid_body: Query<&RigidBody, Allow<Disabled>>,
    settings: Res<AvianSteamAudioSettings>,
) -> Result {
    let (has_sensor, has_material) = collider.get(add.entity)?;
    if has_sensor {
        return Ok(());
    }
    let rigid_body = rigid_body.get(add.entity)?;
    commands
        .entity(add.entity)
        .try_insert(InSteamAudioMeshSpawnQueue);
    if rigid_body.is_static() {
        commands.entity(add.entity).try_insert(Static);
    }
    if settings.auto_insert_materials && !has_material {
        commands
            .entity(add.entity)
            .try_insert(SteamAudioMaterial::default());
    }
    Ok(())
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
            commands
                .entity(entity)
                .try_insert(InSteamAudioMeshSpawnQueue);
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
            Option<&SteamAudioMaterial>,
            &GlobalTransform,
            Has<Static>,
        ),
        With<InSteamAudioMeshSpawnQueue>,
    >,
    mut root: ResMut<SteamAudioRootScene>,
    mut errors: Local<Vec<String>>,
) -> Result {
    errors.clear();
    for (entity, name, collider, material, transform, is_static) in &queued {
        if material.is_none() {
            commands
                .entity(entity)
                .try_insert(SteamAudioMaterial::default());
        }
        let material = material.copied().unwrap_or_default();
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
                let static_mesh = match mesh.to_steam_audio_mesh(&sub_scene, material.into()) {
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
            let static_mesh = match mesh.to_steam_audio_mesh(&root, material.into()) {
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
        #[cfg(feature = "debug")]
        {
            use bevy_steam_audio::debug::SteamAudioGizmo;
            let mesh = match collider.trimesh_builder().build() {
                Ok(mesh) => mesh,
                Err(err) => {
                    errors.push(format!("{name}: Failed to convert mesh: {err}"));
                    continue;
                }
            };
            let gizmo = SteamAudioGizmo {
                vertices: mesh.vertices,
                indices: mesh.indices,
            };
            commands.entity(entity).try_insert(gizmo);
        }
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
    map.retain(|shape, _| Weak::strong_count(shape) > 0);
}

pub trait ToSteamAudioMesh {
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
