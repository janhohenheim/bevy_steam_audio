use std::f32::consts::TAU;

use bevy::{color::palettes::tailwind, prelude::*};
use bevy_seedling::prelude::*;
use bevy_steam_audio::{
    prelude::*,
    scene::mesh_backend::{Mesh3dSteamAudioScenePlugin, SteamAudioMesh},
};

fn main() {
    App::new()
        .add_plugins((
            DefaultPlugins,
            SeedlingPlugin::default(),
            // Add the SteamAudioPlugin to the app to enable Steam Audio functionality
            SteamAudioPlugin::default(),
            // Steam Audio still needs some scene backend to know how to build its 3D scene.
            // Mesh3dSteamAudioScenePlugin does this by using all entities that hold both
            // `Mesh3d` and `MeshMaterial3d`.
            Mesh3dSteamAudioScenePlugin::default(),
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
    commands.spawn((
        Camera3d::default(),
        Transform::from_xyz(0.0, 2.0, 6.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));

    // The sphere is our listener using SteamAudioListener
    commands.spawn((
        SteamAudioListener,
        Transform::default(),
        Mesh3d(meshes.add(Sphere::new(0.5))),
        MeshMaterial3d(materials.add(Color::WHITE)),
    ));

    // Some occluding geometry using MeshSteamAudioMaterial
    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(3.0, 2.0, 0.5))),
        MeshMaterial3d(materials.add(Color::from(tailwind::RED_700).with_alpha(0.7))),
        Transform::from_xyz(0.0, 0.0, -1.0),
        SteamAudioMesh::default(),
    ));

    // The sample player uses Steam Audio through the SteamAudioPool
    commands.spawn((
        SamplePlayer::new(assets.load("selfless_courage.ogg")).looping(),
        SteamAudioPool,
        Mesh3d(meshes.add(Cylinder::new(0.2, 0.001))),
        MeshMaterial3d(materials.add(StandardMaterial {
            unlit: true,
            ..StandardMaterial::from(Color::from(tailwind::GREEN_400))
        })),
        PointLight {
            shadows_enabled: true,
            color: Color::from(tailwind::GREEN_400),
            ..default()
        },
    ));

    commands.spawn((
        DirectionalLight::default(),
        Transform::default().looking_to(Vec3::new(0.5, -1.0, -0.3), Vec3::Y),
    ));
}

// Rotate the sample player around the camera to demonstrate Steam Audio's capabilities
fn rotate_audio(
    mut sample_player: Single<&mut Transform, With<SamplePlayer>>,
    listener: Single<&Transform, (With<SteamAudioListener>, Without<SamplePlayer>)>,
    time: Res<Time>,
) {
    let vertical_speed = 0.75;
    let vertical_angle_frac = (time.elapsed_secs() * vertical_speed).sin();
    let max_vertical_angle = TAU / 8.0;

    let horizontal_speed = 0.1;
    let horizontal_angle = (time.elapsed_secs() * TAU * horizontal_speed) % TAU;

    let horizontal_rotation = Quat::from_rotation_y(horizontal_angle);
    let vertical_rotation = Quat::from_rotation_x(max_vertical_angle * vertical_angle_frac);

    let mut base_position = Transform::from_xyz(0.0, 0.0, -2.0);
    base_position.rotate_around(
        listener.translation,
        horizontal_rotation * vertical_rotation,
    );

    **sample_player = base_position;
    sample_player.look_at(listener.translation, Vec3::Y);
    sample_player.rotate_local_x(TAU / 4.0);
}
