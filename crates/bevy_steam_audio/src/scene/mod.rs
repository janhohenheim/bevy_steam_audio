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
pub struct SteamAudioInstancedMesh(pub audionimbus::InstancedMesh);

#[derive(Component)]
pub struct SteamAudioStaticMesh(pub audionimbus::StaticMesh);

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

fn update_transforms(
    transforms: Query<(Ref<GlobalTransform>, &SteamAudioInstancedMesh)>,
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

#[derive(Component, Default, Reflect)]
#[reflect(Component)]
pub struct InSteamAudioMeshSpawnQueue;
