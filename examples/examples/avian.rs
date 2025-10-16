use avian_steam_audio::prelude::*;
use avian3d::prelude::*;
use bevy::{color::palettes::tailwind, prelude::*};
use bevy_seedling::prelude::*;
use bevy_steam_audio::prelude::*;

fn main() {
    App::new()
        .add_plugins((
            DefaultPlugins,
            SeedlingPlugin::default(),
            PhysicsPlugins::default(),
            SteamAudioPlugin::default(),
            // By using the `AvianSteamAudioScenePlugin`
            AvianSteamAudioScenePlugin,
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
    commands.spawn((Camera3d::default(), SteamAudioListener));

    commands.spawn((
        SamplePlayer::new(assets.load("selfless_courage.ogg")).looping(),
        SteamAudioPool,
        Transform::from_xyz(-1.5, 0.0, -3.0),
        Mesh3d(meshes.add(Sphere::new(0.2))),
        MeshMaterial3d(materials.add(Color::from(tailwind::GREEN_400))),
    ));

    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(0.1, 1.0, 3.0))),
        MeshMaterial3d(materials.add(Color::from(tailwind::GRAY_600))),
        Transform::from_xyz(1.0, 0.0, 0.0),
        SteamAudioMaterial::default(),
        RigidBody::Static,
        ColliderConstructor::TrimeshFromMesh,
    ));

    commands.spawn((
        DirectionalLight::default(),
        Transform::default().looking_to(Vec3::new(0.5, -1.0, -0.3), Vec3::Y),
    ));
}
