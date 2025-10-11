use bevy::{prelude::*, scene::SceneInstanceReady};
use bevy_seedling::prelude::*;
use bevy_steam_audio::{
    SteamAudioSamplePlayer,
    prelude::*,
    scene::mesh_backend::{Mesh3dBackendPlugin, MeshSteamAudioMaterial},
};

mod util;
use util::prelude::*;

fn main() {
    App::new()
        .add_plugins((
            DefaultPlugins,
            SeedlingPlugin::default(),
            SteamAudioPlugin::default(),
            Mesh3dBackendPlugin::default(),
            CameraControllerPlugin,
        ))
        .add_systems(Startup, setup)
        .run();
}

fn setup(mut commands: Commands, assets: Res<AssetServer>) {
    commands
        .spawn(SceneRoot(assets.load("dungeon.glb#Scene0")))
        .observe(set_material);
    commands.spawn((
        Camera3d::default(),
        EnvironmentMapLight {
            diffuse_map: assets.load("environment_maps/voortrekker_interior_1k_diffuse.ktx2"),
            specular_map: assets.load("environment_maps/voortrekker_interior_1k_specular.ktx2"),
            intensity: 2000.0,
            ..default()
        },
        CameraController::default(),
        SteamAudioListener,
    ));
    commands.spawn((
        SamplePlayer::new(assets.load("selfless_courage.ogg")),
        SteamAudioSamplePlayer::default(),
        Transform::from_xyz(0.0, 1.0, 2.0),
        PointLight::default(),
    ));
    commands.spawn((
        DirectionalLight::default(),
        Transform::default().looking_to(Vec3::new(0.5, -1.0, 0.3), Vec3::Y),
    ));
}

fn set_material(
    ready: On<SceneInstanceReady>,
    children: Query<&Children>,
    meshes: Query<(), With<Mesh3d>>,
    mut commands: Commands,
) {
    for child in children.iter_descendants(ready.entity) {
        if meshes.contains(child) {
            commands
                .entity(child)
                .insert(MeshSteamAudioMaterial(SteamAudioMaterial::GENERIC));
        }
    }
}
