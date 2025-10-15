use std::f32::consts::TAU;

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
    // The camera is our listener using SteamAudioListener
    commands.spawn((Camera3d::default(), SteamAudioListener));

    // Reverb is a global effect, simulating how the sounds comes back to the listener from all across the room.
    // Since its effect is global, we don't need a transform on the sample player.
    commands.spawn((
        SamplePlayer::new(assets.load("selfless_courage.ogg")).looping(),
        SteamAudioReverbPool,
    ));

    // To show the reverb, let's put the listener in a room that is open to the right.
    // The effect will be that certain frequencies will be reflected from the left wall and thus some parts of the
    // audio will sounds different on the left speaker.
    for transform in [
        // floor
        Transform::from_xyz(-0.5, -0.5, 0.0),
        // top
        Transform::from_xyz(-0.5, 0.5, 0.0),
        // left wall
        Transform::from_xyz(-0.8, 0.0, 0.0).with_rotation(Quat::from_rotation_z(TAU / 4.0)),
        // front wall
        Transform::from_xyz(-0.5, 0.0, -1.2).with_rotation(Quat::from_rotation_x(TAU / 4.0)),
        // back wall
        Transform::from_xyz(-0.5, 0.0, 1.2).with_rotation(Quat::from_rotation_x(TAU / 4.0)),
    ] {
        commands.spawn((
            Mesh3d(meshes.add(Cuboid::new(2.0, 0.4, 2.0))),
            MeshMaterial3d(materials.add(Color::from(tailwind::GRAY_600))),
            transform,
            SteamAudioMesh::default(),
        ));
    }

    // A little light to make the box less dark :)
    commands.spawn(PointLight::default());
}
