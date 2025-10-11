use std::f32::consts::TAU;

use bevy::prelude::*;
use bevy_seedling::prelude::*;
use bevy_steam_audio::{
    SteamAudioSamplePlayer,
    prelude::*,
    scene::mesh_backend::{Mesh3dBackendPlugin, MeshSteamAudioMaterial},
};

fn main() {
    App::new()
        .add_plugins((
            DefaultPlugins,
            SeedlingPlugin::default(),
            // Add the SteamAudioPlugin to the app to enable Steam Audio functionality
            SteamAudioPlugin::default(),
            // Steam Audio still needs some scene backend to know how to build its 3D scene.
            // Mesh3dBackendPlugin does this by simply using all `Mesh3d`s.
            Mesh3dBackendPlugin::default(),
        ))
        .add_systems(Startup, setup)
        .add_systems(Update, rotate_audio)
        .run();
}

fn setup(
    mut commands: Commands,
    assets: Res<AssetServer>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    // The camera is our listener using  SteamAudioListener
    commands.spawn((Camera3d::default(), SteamAudioListener));

    // Some occluding geometry using MeshSteamAudioMaterial
    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(3.0, 2.0, 0.5))),
        MeshMaterial3d(materials.add(Color::WHITE)),
        Transform::from_xyz(0.0, 0.0, -4.0),
        MeshSteamAudioMaterial(SteamAudioMaterial::GENERIC),
    ));

    // The sample player uses Steam Audio through SteamAudioSamplePlayer
    commands.spawn((
        SamplePlayer::new(assets.load("selfless_courage.ogg")),
        SteamAudioSamplePlayer::default(),
        Transform::from_xyz(6.0, 0.0, 0.0),
        Mesh3d(meshes.add(Sphere::new(0.5))),
        MeshMaterial3d(materials.add(Color::WHITE)),
    ));

    commands.spawn((
        DirectionalLight::default(),
        Transform::default().looking_to(Vec3::new(0.5, -1.0, -0.3), Vec3::Y),
    ));
}

// Rotate the sample player around the camera to demonstrate Steam Audio's capabilities
fn rotate_audio(
    mut sample_player: Single<&mut Transform, With<SamplePlayer>>,
    camera: Single<&Transform, (With<Camera>, Without<SamplePlayer>)>,
    time: Res<Time>,
) {
    let seconds_for_one_orbit = 8.0;
    sample_player.rotate_around(
        camera.translation,
        Quat::from_rotation_y(TAU / seconds_for_one_orbit * time.delta_secs()),
    );
}
