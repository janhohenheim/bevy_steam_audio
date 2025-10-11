# Bevy Steam Audio

WIP of an integration between Bevy and Steam Audio via audionimbus. See https://github.com/MaxenceMaire/audionimbus-demo for a minimal POC of the approach used in the crate.

## Usage

```rust
use bevy::prelude::*;
use bevy_seedling::prelude::*;
use bevy_steam_audio::{
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
            // Mesh3dBackendPlugin does this by using all entities that hold both
            // `Mesh3d` and `MeshMaterial3d`.
            Mesh3dBackendPlugin::default(),
        ))
        .add_systems(Startup, setup)
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

    // The sample player uses Steam Audio through the SteamAudioPool
    commands.spawn((
        SamplePlayer::new(assets.load("selfless_courage.ogg")),
        SteamAudioPool,
        Transform::from_xyz(6.0, 0.0, 0.0),
        Mesh3d(meshes.add(Sphere::new(0.5))),
        MeshMaterial3d(materials.add(Color::WHITE)),
    ));

    commands.spawn((
        DirectionalLight::default(),
        Transform::default().looking_to(Vec3::new(0.5, -1.0, -0.3), Vec3::Y),
    ));
}
```

## Compatibility

| Bevy | bevy_steam_audio | Steam Audio |
|------|------------------|-------------|
| 0.17 | 0.1              | 4.7         |
