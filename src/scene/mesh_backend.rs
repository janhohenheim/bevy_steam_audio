use std::marker::PhantomData;

use bevy_ecs::entity_disabling::Disabled;
use bevy_platform::collections::HashMap;

use crate::{
    prelude::*,
    scene::SteamAudioRootScene,
    simulation::AudionimbusSimulator,
    wrapper::{ToSteamAudioMesh as _, ToSteamAudioTransform},
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
                update_transforms,
            )
                .chain()
                .in_set(SteamAudioSystems::MeshLifecycle),
        );
        app.add_observer(remove_mesh_with_material)
            .add_observer(remove_dynamic_mesh_from_scene)
            .add_observer(remove_static_mesh_from_scene);
        app.init_resource::<MeshToScene>()
            .init_resource::<ToSpawn>();
    }
}

#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct SteamAudioMesh {
    pub dynamic: bool,
    pub material: SteamAudioMaterial,
}

impl Default for SteamAudioMesh {
    fn default() -> Self {
        Self {
            dynamic: true,
            material: SteamAudioMaterial::default(),
        }
    }
}

#[derive(Resource, Default, Deref, DerefMut)]
struct MeshToScene(HashMap<AssetId<Mesh>, audionimbus::Scene>);

#[derive(Resource, Default, Deref, DerefMut)]
struct ToSpawn(Vec<Entity>);

#[derive(Component)]
struct InstancedMesh(audionimbus::InstancedMesh);

#[derive(Component)]
struct StaticMesh(audionimbus::StaticMesh);

fn queue_steam_audio_mesh_processing(
    meshes: Query<(Entity, Ref<Mesh3d>), With<SteamAudioMesh>>,
    mut to_spawn: ResMut<ToSpawn>,
) {
    for (entity, mesh) in meshes.iter() {
        if mesh.is_changed() {
            to_spawn.push(entity);
        }
    }
}

fn remove_mesh_with_material(remove: On<Remove, SteamAudioMesh>, mut commands: Commands) {
    commands
        .entity(remove.entity)
        .try_remove::<InstancedMesh>()
        .try_remove::<StaticMesh>();
}

fn remove_dynamic_mesh_from_scene(
    remove: On<Replace, InstancedMesh>,
    instanced_mesh: Query<&InstancedMesh, Allow<Disabled>>,
    mut root: ResMut<SteamAudioRootScene>,
) -> Result {
    let instanced_mesh = instanced_mesh.get(remove.entity)?;
    // replace runs *before* the actual replace, so let's remove the *old* mesh
    root.remove_instanced_mesh(&instanced_mesh.0);
    Ok(())
}

fn remove_static_mesh_from_scene(
    remove: On<Replace, StaticMesh>,
    static_mesh: Query<&StaticMesh, Allow<Disabled>>,
    mut root: ResMut<SteamAudioRootScene>,
) -> Result {
    let static_mesh = static_mesh.get(remove.entity)?;
    // replace runs *before* the actual replace, so let's remove the *old* mesh
    root.remove_static_mesh(&static_mesh.0);
    Ok(())
}

fn spawn_new_steam_audio_meshes(
    mut commands: Commands,
    mut to_add: ResMut<ToSpawn>,
    mut map: ResMut<MeshToScene>,
    mesh_handles: Query<(&Mesh3d, &SteamAudioMesh, &GlobalTransform), Allow<Disabled>>,
    meshes: Res<Assets<Mesh>>,
    mut root: ResMut<SteamAudioRootScene>,
    mut errors: Local<Vec<String>>,
    names: Query<NameOrEntity>,
) -> Result {
    errors.clear();
    if to_add.is_empty() {
        return Ok(());
    }

    // We don't actually need the simulator, but calling `root.commit()` is not allowed while a simulation is running.

    to_add.retain(|entity| {
        let name = names.get(*entity).unwrap();
        let Ok((mesh_handle, material, transform)) = mesh_handles.get(*entity) else {
            errors.push(format!(
                "{name}: MeshSteamAudioMaterial was added to an entity without Mesh3D"
            ));
            return false;
        };
        let id = mesh_handle.id();
        let Some(mesh) = meshes.get(id) else {
            // mesh not loaded yet
            return true;
        };

        if material.dynamic {
            let sub_scene = if let Some(sub_scene) = map.get(&id) {
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
                        return false;
                    }
                };
                let static_mesh = match mesh.to_steam_audio_mesh(&sub_scene, material.material) {
                    Ok(mesh) => mesh,
                    Err(err) => {
                        errors.push(format!("{name}: Failed to convert mesh: {err}"));
                        return false;
                    }
                };
                sub_scene.add_static_mesh(static_mesh);
                // committing a new scene should be fine during simulation of a different scene
                sub_scene.commit();
                map.insert(id, sub_scene.clone());
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
                        return false;
                    }
                };
            info!("Instanced mesh created");
            root.add_instanced_mesh(instanced_mesh.clone());
            commands
                .entity(*entity)
                .try_insert(InstancedMesh(instanced_mesh));
        } else {
            let mesh = mesh.clone().transformed_by(transform.compute_transform());
            let static_mesh = match mesh.to_steam_audio_mesh(&root, material.material) {
                Ok(mesh) => mesh,
                Err(err) => {
                    errors.push(format!("{name}: Failed to convert mesh: {err}"));
                    return false;
                }
            };
            root.add_static_mesh(static_mesh);
        }

        false
    });
    // Do not call root.commit(), it's not safe during running simulations

    if !errors.is_empty() {
        Err(errors.join("\n").into())
    } else {
        Ok(())
    }
}

fn garbage_collect_meshes(
    mut asset_events: MessageReader<AssetEvent<Mesh>>,
    mut map: ResMut<MeshToScene>,
) {
    for event in asset_events.read() {
        if let AssetEvent::Removed { id } | AssetEvent::Modified { id } = event {
            map.remove(id);
        }
    }
}

fn update_transforms(
    transforms: Query<(Ref<GlobalTransform>, &InstancedMesh)>,
    mut root: ResMut<SteamAudioRootScene>,
) {
    for (transform, instanced_mesh) in transforms.iter() {
        if !transform.is_changed() {
            continue;
        }
        let transform = transform.to_steam_audio_transform();

        root.update_instanced_mesh_transform(&instanced_mesh.0, transform);
    }
}
