use avian_steam_audio::AvianSteamAudioScenePlugin;
use avian3d::PhysicsPlugins;
use bevy::{camera::Exposure, color::palettes::tailwind, prelude::*};
use bevy_seedling::prelude::*;
use bevy_steam_audio::prelude::*;
use bevy_trenchbroom::{physics::SceneCollidersReady, prelude::*};
use trenchbroom_steam_audio::prelude::*;

use crate::util::prelude::{CameraController, CameraControllerPlugin};

mod util;

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
            // The debug plugin displays the audio meshes with a different color for each material
            SteamAudioDebugPlugin,
            CameraControllerPlugin,
        ))
        .insert_resource(
            // The default settings already add some useful mappings, e.g. to make all textures that contain the name "wood" wooden materials.
            // This resource is added for you, but we add it explicitly here to extend the default settings.
            TrenchBroomSteamAudioSettings::default()
                .map_material("*moss*", SteamAudioMaterial::CARPET),
        )
        .add_systems(Startup, setup)
        .add_observer(setup_loud_speaker)
        .add_observer(bake_paths)
        .run();
}

fn setup(mut commands: Commands, assets: Res<AssetServer>) {
    // The camera is our listener using SteamAudioListener
    commands.spawn((
        Camera3d::default(),
        SteamAudioListener,
        CameraController::default(),
        Exposure::INDOOR,
    ));

    commands.spawn(SceneRoot(assets.load("maps/trenchbroom_demo.map#Scene")));
}

#[point_class(base(Transform))]
struct LoudSpeaker;

fn setup_loud_speaker(
    add: On<Add, LoudSpeaker>,
    mut commands: Commands,
    assets: Res<AssetServer>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    commands
        .entity(add.entity)
        .try_insert((
            SamplePlayer::new(assets.load("selfless_courage.ogg")).looping(),
            SteamAudioPool,
            sample_effects![SteamAudioNode {
                direct_gain: 0.0,
                reflection_gain: 0.0,
                ..default()
            }],
            PointLight {
                shadows_enabled: true,
                ..default()
            },
        ))
        .with_child((
            Transform::default(),
            Visibility::default(),
            Mesh3d(meshes.add(Sphere::new(0.2))),
            MeshMaterial3d(materials.add(StandardMaterial {
                unlit: true,
                ..StandardMaterial::from(Color::from(tailwind::GREEN_400))
            })),
        ));
}

fn bake_paths(_ready: On<SceneCollidersReady>, mut generate_probes: MessageWriter<GenerateProbes>) {
    generate_probes.write(GenerateProbes::default());
}
