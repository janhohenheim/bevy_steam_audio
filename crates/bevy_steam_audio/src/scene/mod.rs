use bevy_ecs::entity_disabling::Disabled;

use crate::{STEAM_AUDIO_CONTEXT, prelude::*};

pub mod mesh_backend;

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<SteamAudioRootScene>();
    app.add_observer(remove_static)
        .add_observer(add_static)
        .add_observer(remove_material)
        .add_observer(remove_dynamic_mesh_from_scene)
        .add_observer(remove_static_mesh_from_scene);
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

#[derive(Component, Debug, Clone, Copy, PartialEq, Reflect)]
#[reflect(Component)]
pub struct Static;

#[derive(Component)]
pub struct SteamAudioInstancedMesh(audionimbus::InstancedMesh);

#[derive(Component)]
pub struct SteamAudioStaticMesh(audionimbus::StaticMesh);

fn remove_material(remove: On<Remove, SteamAudioMaterial>, mut commands: Commands) {
    commands
        .entity(remove.entity)
        .try_remove::<SteamAudioInstancedMesh>()
        .try_remove::<SteamAudioStaticMesh>();
}

fn remove_static(remove: On<Remove, Static>, mut commands: Commands) {
    commands
        .entity(remove.entity)
        .try_remove::<SteamAudioStaticMesh>();
}

fn add_static(remove: On<Add, Static>, mut commands: Commands) {
    commands
        .entity(remove.entity)
        .try_remove::<SteamAudioInstancedMesh>();
}

fn remove_dynamic_mesh_from_scene(
    remove: On<Replace, SteamAudioInstancedMesh>,
    instanced_mesh: Query<&SteamAudioInstancedMesh, Allow<Disabled>>,
    mut root: ResMut<SteamAudioRootScene>,
) -> Result {
    let instanced_mesh = instanced_mesh.get(remove.entity)?;
    // replace runs *before* the actual replace, so let's remove the *old* mesh
    root.remove_instanced_mesh(&instanced_mesh.0);
    Ok(())
}

fn remove_static_mesh_from_scene(
    remove: On<Replace, SteamAudioStaticMesh>,
    static_mesh: Query<&SteamAudioStaticMesh, Allow<Disabled>>,
    mut root: ResMut<SteamAudioRootScene>,
) -> Result {
    let static_mesh = static_mesh.get(remove.entity)?;
    // replace runs *before* the actual replace, so let's remove the *old* mesh
    root.remove_static_mesh(&static_mesh.0);
    Ok(())
}
