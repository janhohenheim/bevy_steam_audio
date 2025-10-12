use std::f32::consts::TAU;

use audionimbus::SimulationFlags;
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
            SteamAudioPlugin::default(),
            Mesh3dBackendPlugin::default(),
        ))
        // SteamAudioQuality can be used to set global quality settings.
        // This resource can also be changed at runtime, e.g. in a settings menu.
        .insert_resource(SteamAudioQuality {
            order: 3,
            frame_size: 1024,
            num_bounces: 32,
            direct: SteamAudioDirectQuality {
                max_num_occlusion_samples: 30,
            },
            ..default()
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
) {
    commands.spawn((Camera3d::default(), SteamAudioListener));

    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(3.0, 2.0, 0.5))),
        MeshMaterial3d(materials.add(Color::WHITE)),
        Transform::from_xyz(0.0, 0.0, -4.0),
        SteamAudioMesh::default(),
    ));

    commands.spawn((
        SamplePlayer::new(assets.load("selfless_courage.ogg")),
        Transform::from_xyz(6.0, 0.0, 0.0),
        Mesh3d(meshes.add(Sphere::new(0.5))),
        MeshMaterial3d(materials.add(Color::WHITE)),
        SteamAudioPool,
        // `SteamAudioSamplePlayer` provides per-sample-player configuration
        SteamAudioSamplePlayer {
            // This source generates both direct and reflected sound
            flags: SimulationFlags::DIRECT | SimulationFlags::REFLECTIONS,
        },
        // The `SteamAudioNode` tunes the parameters used when processing the audio.
        sample_effects![SteamAudioNode {
            // boost the reflected sound relative to the direct sound
            direct_gain: 0.1,
            reflection_gain: 3.0,
            // reverb is a kind of reflection, so it's enabled for this sampler by the flags above.
            // but we can disable it by setting the gain to zero
            reverb_gain: 0.0,
            ..default()
        }],
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
