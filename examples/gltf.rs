use bevy::{color::palettes::tailwind, prelude::*, scene::SceneInstanceReady};
use bevy_seedling::prelude::*;
use bevy_steam_audio::{
    prelude::*,
    scene::mesh_backend::{Mesh3dBackendPlugin, MeshSteamAudioMaterial},
};

mod util;
use util::prelude::*;

fn main() {
    App::new()
        .add_plugins((
            DefaultPlugins,
            SeedlingPlugin::default(),
            SteamAudioPlugin::default(),
            Mesh3dBackendPlugin::default(),
            CameraControllerPlugin,
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
    let audio_pos = Transform::from_xyz(40.0, 12.0, 0.0);

    commands
        .spawn(SceneRoot(assets.load("dungeon.glb#Scene0")))
        .observe(set_material);
    commands.spawn((
        Camera3d::default(),
        EnvironmentMapLight {
            diffuse_map: assets.load("environment_maps/voortrekker_interior_1k_diffuse.ktx2"),
            specular_map: assets.load("environment_maps/voortrekker_interior_1k_specular.ktx2"),
            intensity: 2000.0,
            ..default()
        },
        CameraController::default(),
        Transform::from_xyz(18.0, 12.0, 0.0).looking_at(audio_pos.translation, Vec3::Y),
        SteamAudioListener,
    ));
    commands.spawn((
        SamplePlayer::new(assets.load("selfless_courage.ogg")),
        SteamAudioPool,
        sample_effects![SteamAudioNode {
            // TODO: this is just for testing, revert it to only `reverb_gain: 0.01` with no comment :)
            // boost the reflected sound relative to the direct sound
            direct_gain: 1.0,
            reflection_gain: 0.0,
            // reverb is a kind of reflection, so it's enabled for this sampler by the flags above.
            // but we can disable it by setting the gain to zero
            reverb_gain: 0.0,
            pathing_gain: 1.0,
            ..default()
        }],
        audio_pos,
        Mesh3d(meshes.add(Sphere::new(0.5))),
        MeshMaterial3d(materials.add(Color::from(tailwind::GREEN_400).with_alpha(0.5))),
        PointLight::default(),
    ));
    commands.spawn((
        DirectionalLight::default(),
        Transform::default().looking_to(Vec3::new(0.5, -1.0, 0.3), Vec3::Y),
    ));
}

fn set_material(
    ready: On<SceneInstanceReady>,
    children: Query<&Children>,
    meshes: Query<(), (With<Mesh3d>, Without<SamplePlayer>)>,
    mut commands: Commands,
) {
    for child in children.iter_descendants(ready.entity) {
        if meshes.contains(child) {
            commands
                .entity(child)
                .insert(MeshSteamAudioMaterial(SteamAudioMaterial::GENERIC));
        }
    }
    //commands.trigger(GenerateProbes::default());
}
