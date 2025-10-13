use bevy::{color::palettes::tailwind, prelude::*};
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
    commands.spawn((
        SamplerPool(MyOwnSteamAudioPool),
        VolumeNodeConfig {
            channels: NonZeroChannelCount::new(quality.num_channels()).unwrap(),
        },
        sample_effects![SteamAudioNode::default()],
    ));
}

// This is the exact same setup as in minimal.rs, but with `MyOwnSteamAudioPool` instead of `SteamAudioPool`.
fn setup(
    mut commands: Commands,
    assets: Res<AssetServer>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    // The camera is our listener using SteamAudioListener
    commands.spawn((Camera3d::default(), SteamAudioListener));

    // The sample player uses Steam Audio through the SteamAudioPool
    // Let's place it to the front left of the listener, making direct sound come from the left
    commands.spawn((
        SamplePlayer::new(assets.load("selfless_courage.ogg")).looping(),
        MyOwnSteamAudioPool,
        Transform::from_xyz(-1.5, 0.0, -3.0),
        Mesh3d(meshes.add(Sphere::new(0.2))),
        MeshMaterial3d(materials.add(Color::from(tailwind::GREEN_400))),
    ));

    // Some occluding geometry using MeshSteamAudioMaterial
    // Let's place it to the right of the listener, making reflected sound come from the right
    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(0.1, 1.0, 3.0))),
        MeshMaterial3d(materials.add(Color::from(tailwind::GRAY_600))),
        Transform::from_xyz(1.0, 0.0, 0.0),
        SteamAudioMesh::default(),
    ));

    commands.spawn((
        DirectionalLight::default(),
        Transform::default().looking_to(Vec3::new(0.5, -1.0, -0.3), Vec3::Y),
    ));
}
