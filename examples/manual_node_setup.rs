use std::f32::consts::TAU;

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
        .add_systems(Startup, (spawn_custom_pool, setup))
        .add_systems(Update, rotate_audio)
        // Make sure to require `SteamAudioSamplePlayer` or you won't be able to hear anything
        .register_required_components::<MyOwnSteamAudioPool, SteamAudioSamplePlayer>()
        .run();
}

#[derive(PoolLabel, PartialEq, Eq, Debug, Hash, Clone, Default)]
pub struct MyOwnSteamAudioPool;

// This is a recreation of the `SteamAudioPool`.
// By creating a custom pool, you can add any custom audio processing nodes you want into the mix.
fn spawn_custom_pool(mut commands: Commands, quality: Res<SteamAudioQuality>) {
    // Copy-paste this part if you want to set up your own pool!
    commands
        .spawn((
            SamplerPool(MyOwnSteamAudioPool),
            VolumeNodeConfig {
                channels: NonZeroChannelCount::new(quality.num_channels()).unwrap(),
            },
            sample_effects![SteamAudioNode::default()],
        ))
        .connect(SteamAudioDecodeBus);
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
        SamplePlayer::new(assets.load("selfless_courage.ogg")).looping(),
        Transform::from_xyz(6.0, 0.0, 0.0),
        Mesh3d(meshes.add(Sphere::new(0.5))),
        MeshMaterial3d(materials.add(Color::WHITE)),
        MyOwnSteamAudioPool,
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
