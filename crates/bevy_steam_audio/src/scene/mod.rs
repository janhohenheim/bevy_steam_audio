use bevy_ecs::entity_disabling::Disabled;

use crate::{STEAM_AUDIO_CONTEXT, prelude::*, wrapper::ToSteamAudioTransform as _};

pub mod mesh_backend;

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<SteamAudioRootScene>();
    app.add_observer(remove_material)
        .add_observer(remove_dynamic_mesh_from_scene)
        .add_observer(remove_static_mesh_from_scene);
    app.add_systems(
        PostUpdate,
        update_transforms.in_set(SteamAudioSystems::UpdateTransforms),
    );
}

#[derive(Resource, Deref, DerefMut)]
pub struct SteamAudioRootScene(pub audionimbus::Scene<'static, audionimbus::DefaultRayTracer>);

impl Default for SteamAudioRootScene {
    fn default() -> Self {
        let mut scene = audionimbus::Scene::try_new(&STEAM_AUDIO_CONTEXT).unwrap();
        scene.commit();
        Self(scene)
    }
}

#[derive(Component, Debug, Clone, Copy, PartialEq, Reflect)]
#[reflect(Component)]
pub struct Static;

/// Stores the handle returned by `Scene::add_instanced_mesh`.
#[derive(Component)]
pub struct SteamAudioInstancedMesh(pub audionimbus::InstancedMeshHandle);

/// Stores the handle returned by `Scene::add_static_mesh`.
#[derive(Component)]
pub struct SteamAudioStaticMesh(pub audionimbus::StaticMeshHandle);

fn remove_material(remove: On<Remove, SteamAudioMaterial>, mut commands: Commands) {
    commands
        .entity(remove.entity)
        .try_remove::<SteamAudioInstancedMesh>()
        .try_remove::<SteamAudioStaticMesh>();
}

fn remove_dynamic_mesh_from_scene(
    remove: On<Replace, SteamAudioInstancedMesh>,
    instanced_mesh: Query<&SteamAudioInstancedMesh, Allow<Disabled>>,
    mut root: ResMut<SteamAudioRootScene>,
) -> Result {
    let handle = instanced_mesh.get(remove.entity)?;
    root.0.remove_instanced_mesh(handle.0);
    Ok(())
}

fn remove_static_mesh_from_scene(
    remove: On<Replace, SteamAudioStaticMesh>,
    static_mesh: Query<&SteamAudioStaticMesh, Allow<Disabled>>,
    mut root: ResMut<SteamAudioRootScene>,
) -> Result {
    let handle = static_mesh.get(remove.entity)?;
    root.0.remove_static_mesh(handle.0);
    Ok(())
}

fn update_transforms(
    transforms: Query<(Ref<GlobalTransform>, &SteamAudioInstancedMesh)>,
    mut root: ResMut<SteamAudioRootScene>,
) {
    for (transform, handle) in transforms.iter() {
        if !transform.is_changed() {
            continue;
        }
        let transform = transform.to_steam_audio_transform();
        root.0.update_instanced_mesh_transform(handle.0, transform);
    }
}

#[derive(Component, Default, Reflect)]
#[reflect(Component)]
pub struct InSteamAudioMeshSpawnQueue;
