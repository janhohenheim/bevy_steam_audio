use std::time::Duration;

use bevy::{
    color::palettes::tailwind, input::common_conditions::input_just_pressed, prelude::*,
    time::common_conditions::on_timer,
};
use bevy_seedling::prelude::*;
use bevy_steam_audio::{prelude::*, scene::mesh_backend::Mesh3dSteamAudioScenePlugin};

fn main() {
    App::new()
        .add_plugins((
            DefaultPlugins,
            SeedlingPlugin::default(),
            // Add the SteamAudioPlugin to the app to enable Steam Audio functionality
            SteamAudioPlugin::default(),
            // Steam Audio still needs some scene backend to know how to build its 3D scene.
            // Mesh3dSteamAudioScenePlugin does this by using all entities that hold both
            // `Mesh3d` and `SteamAudioMaterial`.
            Mesh3dSteamAudioScenePlugin::default(),
        ))
        .add_systems(Startup, setup)
        .add_systems(Update, spawn.run_if(on_timer(Duration::from_millis(200))))
        .init_resource::<Step>()
        .run();
}

fn setup(mut commands: Commands) {
    // The camera is our listener using SteamAudioListener
    commands.spawn((Camera3d::default(), SteamAudioListener));
}

#[derive(Resource)]
struct Step(Handle<AudioSample>);

impl FromWorld for Step {
    fn from_world(world: &mut World) -> Self {
        let assets = world.resource::<AssetServer>();
        Self(assets.load("step1.ogg"))
    }
}

fn spawn(mut commands: Commands, step: Res<Step>) {
    commands.spawn((
        SamplePlayer::new(step.0.clone()),
        SteamAudioPool,
        Transform::from_xyz(-1.5, 0.0, -3.0),
    ));
}
