use std::time::Duration;

use avian_steam_audio::prelude::*;
use avian3d::prelude::*;
use bevy::{color::palettes::tailwind, prelude::*, time::common_conditions::on_timer};
use bevy_seedling::prelude::*;
use bevy_steam_audio::prelude::*;

fn main() {
    App::new()
        .add_plugins((
            DefaultPlugins,
            SeedlingPlugin::default(),
            PhysicsPlugins::default(),
            SteamAudioPlugin::default(),
            // By using the `AvianSteamAudioScenePlugin`, all `Colliders` that belong to a `RigidBody` will be automatically treated as acoustic objects.
            AvianSteamAudioScenePlugin,
        ))
        .add_systems(Startup, setup)
        .add_systems(
            Update,
            apply_force.run_if(on_timer(Duration::from_secs_f32(0.5))),
        )
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
        Transform::from_xyz(0.0, 0.2, -1.0).looking_at(Vec3::new(0.0, 0.2, -2.0), Vec3::Y),
        SteamAudioListener,
    ));

    let sphere = Sphere::new(0.03);
    commands.spawn((
        SamplePlayer::new(assets.load("selfless_courage.ogg")).looping(),
        SteamAudioPool,
        Transform::from_xyz(0.0, 0.0, -3.0),
        Mesh3d(meshes.add(sphere.clone())),
        MeshMaterial3d(materials.add(Color::from(tailwind::GREEN_400))),
        // The source doesn't need to be a rigid body or have a collider, but let's add some so the
        // cubes don't fall through it. Since we don't want to trap the sound inside the ball, let's
        // also add NotSteamAudioCollider to make sure the sound goes *through* this collider.
        NotSteamAudioCollider,
        RigidBody::Static,
        Collider::from(sphere),
    ));

    let floor = Cuboid::new(4.0, 0.2, 4.0);
    commands.spawn((
        // All colliders belonging to a rigid body are implicitly also Steam Audio materials.
        Mesh3d(meshes.add(floor)),
        MeshMaterial3d(materials.add(Color::from(tailwind::GRAY_600))),
        Transform::from_xyz(0.0, -0.5, -2.0),
        RigidBody::Static,
        Collider::from(floor),
    ));

    let cube = Cuboid::new(0.3, 0.3, 0.3);
    let mesh = meshes.add(cube);
    let collider = Collider::from(cube);

    for i in 0..6 {
        for j in 0..4 {
            commands.spawn((
                Mesh3d(mesh.clone()),
                MeshMaterial3d(materials.add(Color::from(tailwind::SLATE_100))),
                Transform::from_xyz(i as f32 * 0.31 - 0.7, j as f32 * 0.31, -2.0),
                RigidBody::Dynamic,
                collider.clone(),
                // We can optionally use a specific material for each collider instead of the default material.
                SteamAudioMaterial::WOOD,
            ));
        }
    }
    commands.spawn((
        DirectionalLight::default(),
        Transform::default().looking_to(Vec3::new(0.5, -1.0, -0.3), Vec3::Y),
    ));
}

fn apply_force(mut forces: Query<Forces>) {
    for mut force in forces.iter_mut() {
        force.apply_linear_impulse(Vec3::Y * 0.1);
    }
}
