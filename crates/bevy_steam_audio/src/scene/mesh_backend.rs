use std::marker::PhantomData;

use bevy_platform::collections::HashMap;

use crate::{
    prelude::*,
    scene::{
        InSteamAudioMeshSpawnQueue, Static, SteamAudioInstancedMesh, SteamAudioRootScene,
        SteamAudioStaticMesh,
    },
    wrapper::{ToSteamAudioMesh as _, ToSteamAudioTransform},
};

pub struct Mesh3dSteamAudioScenePlugin {
    _pd: PhantomData<()>,
}

impl Default for Mesh3dSteamAudioScenePlugin {
    fn default() -> Self {
        Self { _pd: PhantomData }
    }
}

impl Plugin for Mesh3dSteamAudioScenePlugin {
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
        app.init_resource::<MeshToScene>();
    }
}

#[derive(Resource, Default, Deref, DerefMut)]
struct MeshToScene(HashMap<AssetId<Mesh>, audionimbus::Scene>);

fn queue_steam_audio_mesh_processing(
    meshes: Query<(Entity, Ref<Mesh3d>), With<SteamAudioMaterial>>,
    mut commands: Commands,
) {
    for (entity, mesh) in meshes.iter() {
        if mesh.is_changed() {
            commands
                .entity(entity)
                .try_insert(InSteamAudioMeshSpawnQueue);
        }
    }
}

fn spawn_new_steam_audio_meshes(
    mut commands: Commands,
    mut map: ResMut<MeshToScene>,
    queued: Query<
        (
            Entity,
            NameOrEntity,
            &Mesh3d,
            &SteamAudioMaterial,
            &GlobalTransform,
            Has<Static>,
        ),
        With<InSteamAudioMeshSpawnQueue>,
    >,
    meshes: Res<Assets<Mesh>>,
    mut root: ResMut<SteamAudioRootScene>,
    mut errors: Local<Vec<String>>,
) -> Result {
    errors.clear();
    for (entity, name, mesh_handle, material, transform, is_static) in &queued {
        let id = mesh_handle.id();
        let Some(mesh) = meshes.get(id) else {
            // mesh not loaded yet
            continue;
        };

        if !is_static {
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
            let mesh = mesh.clone().transformed_by(transform.compute_transform());
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

        #[cfg(feature = "debug")]
        {
            use itertools::Itertools as _;

            let Some(vertices) = mesh
                .attribute(Mesh::ATTRIBUTE_POSITION)
                .and_then(|p| p.as_float3())
            else {
                errors.push(format!("{name}: Mesh has no positions"));
                continue;
            };

            let Some(indices) = mesh.indices() else {
                errors.push(format!("{name}: Mesh has no indices"));
                continue;
            };
            let gizmo = SteamAudioGizmo {
                vertices: vertices.iter().map(|v| Vec3::from_array(*v)).collect(),
                indices: indices
                    .iter()
                    .chunks(3)
                    .into_iter()
                    .map(|mut chunk| {
                        [
                            chunk.next().unwrap() as u32,
                            chunk.next().unwrap() as u32,
                            chunk.next().unwrap() as u32,
                        ]
                    })
                    .collect(),
            };
            commands.entity(entity).insert(gizmo);
        }
    }
    // Do not call root.commit(), it's not safe while simulations are running

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
