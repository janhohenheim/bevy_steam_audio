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
            SteamAudioPlugin::default(),
            Mesh3dSteamAudioScenePlugin::default(),
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

    // The sample player uses Steam Audio through the SteamAudioPool
    // Let's place it to the front left of the listener, making direct sound come from the left
    commands.spawn((
        SamplePlayer::new(assets.load("selfless_courage.ogg")).looping(),
        SteamAudioPool,
        // The `SteamAudioNode` tunes the parameters used when processing the audio.
        sample_effects![SteamAudioNode {
            direct_gain: 3.0,
            reflection_gain: 0.5,
            ..default()
        }],
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
