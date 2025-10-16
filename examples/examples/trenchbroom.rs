use avian_steam_audio::AvianSteamAudioScenePlugin;
use avian3d::PhysicsPlugins;
use bevy::{color::palettes::tailwind, prelude::*};
use bevy_seedling::prelude::*;
use bevy_steam_audio::prelude::*;
use bevy_trenchbroom::prelude::*;
use trenchbroom_steam_audio::prelude::*;

fn main() {
    App::new()
        .add_plugins((
            DefaultPlugins,
            SeedlingPlugin::default(),
            PhysicsPlugins::default(),
            SteamAudioPlugin::default(),
            AvianSteamAudioScenePlugin,
            TrenchBroomSteamAudioScenePlugin,
            TrenchBroomPlugins(
                TrenchBroomConfig::new("trenchbroom_steam_audio demo")
                    .assets_path("examples/assets")
                    .default_solid_spawn_hooks(|| {
                        SpawnHooks::new()
                            .convex_collider()
                            .smooth_by_default_angle()
                    }),
            ),
            SteamAudioDebugPlugin,
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

    // The sample player uses Steam Audio through the SteamAudioPool
    // Let's place it to the front left of the listener, making direct sound come from the left
    commands.spawn((
        SamplePlayer::new(assets.load("selfless_courage.ogg")).looping(),
        SteamAudioPool,
        Transform::from_xyz(-1.5, 0.0, -3.0),
        Mesh3d(meshes.add(Sphere::new(0.2))),
        MeshMaterial3d(materials.add(Color::from(tailwind::GREEN_400))),
    ));

    // Some occluding geometry using MeshSteamAudioMaterial
    // Let's place it to the right of the listener, making reflected sound come from the right
    commands.spawn(SceneRoot(assets.load("maps/trenchbroom_demo.map#Scene")));

    commands.spawn((
        DirectionalLight::default(),
        Transform::default().looking_to(Vec3::new(0.5, -1.0, -0.3), Vec3::Y),
    ));
}
