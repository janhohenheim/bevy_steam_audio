use std::{f32::consts::TAU, time::Duration};

use bevy::prelude::*;
use bevy_seedling::prelude::*;
use bevy_steam_audio::{
    prelude::*,
    scene::mesh_backend::{Mesh3dBackendPlugin, SteamAudioMesh},
};

fn main() {
    App::new()
        .add_plugins((
            DefaultPlugins,
            SeedlingPlugin::default(),
            // Add the SteamAudioPlugin to the app to enable Steam Audio functionality
            SteamAudioPlugin::default(),
            // Steam Audio still needs some scene backend to know how to build its 3D scene.
            // Mesh3dBackendPlugin does this by using all entities that hold both
            // `Mesh3d` and `MeshMaterial3d`.
            Mesh3dBackendPlugin::default(),
        ))
        .insert_resource(SteamAudioQuality {
            order: 2,
            frame_size: 1024,
            reflections: SteamAudioReflectionsQuality {
                max_num_sources: 256,
                impulse_duration: Duration::from_secs_f32(1.0),
                // num_rays: 128,
                // kind: bevy_steam_audio::settings::SteamAudioReflectionKind::Hybrid,
                ..Default::default()
            },
            ..Default::default()
        })
        .add_systems(Startup, setup)
        .add_systems(Update, rotate_audio)
        .run();
}

fn setup(
    mut commands: Commands,
    assets: Res<AssetServer>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut main_bus: Single<&mut VolumeNode, With<MainBus>>,
) {
    // The camera is our listener using  SteamAudioListener
    commands.spawn((Camera3d::default(), SteamAudioListener));

    // Some occluding geometry using MeshSteamAudioMaterial
    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(3.0, 2.0, 0.5))),
        MeshMaterial3d(materials.add(Color::WHITE)),
        Transform::from_xyz(0.0, 0.0, -4.0),
        SteamAudioMesh::default(),
    ));

    // turn that shit way down
    // main_bus.volume = Volume::Linear(0.1);

    let total = 1;

    let mut transform = Transform::from_xyz(6.0, 0.0, 0.0);
    for _ in 0..total {
        transform.rotate_around(Vec3::ZERO, Quat::from_rotation_y(TAU / total as f32));

        // The sample player uses Steam Audio through the SteamAudioPool
        commands.spawn((
            SamplePlayer::new(assets.load("selfless_courage.ogg")),
            SteamAudioPool,
            transform,
            Mesh3d(meshes.add(Sphere::new(0.5))),
            MeshMaterial3d(materials.add(Color::WHITE)),
        ));
    }

    commands.spawn((
        DirectionalLight::default(),
        Transform::default().looking_to(Vec3::new(0.5, -1.0, -0.3), Vec3::Y),
    ));
}

// Rotate the sample player around the camera to demonstrate Steam Audio's capabilities
fn rotate_audio(
    mut sample_players: Query<&mut Transform, With<SamplePlayer>>,
    camera: Single<&Transform, (With<Camera>, Without<SamplePlayer>)>,
    time: Res<Time>,
) {
    let seconds_for_one_orbit = 8.0;

    for mut sample_player in sample_players.iter_mut() {
        sample_player.rotate_around(
            camera.translation,
            Quat::from_rotation_y(TAU / seconds_for_one_orbit * time.delta_secs()),
        );
    }
}
