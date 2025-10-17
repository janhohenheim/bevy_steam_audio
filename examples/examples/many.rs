use std::time::Duration;

use bevy::{prelude::*, time::common_conditions::on_timer};
use bevy_seedling::prelude::*;
use bevy_steam_audio::{prelude::*, scene::mesh_backend::Mesh3dSteamAudioScenePlugin};

fn main() {
    App::new()
        .add_plugins((
            DefaultPlugins,
            SeedlingPlugin::default(),
            SteamAudioPlugin::default(),
            Mesh3dSteamAudioScenePlugin::default(),
        ))
        .add_systems(Startup, setup)
        .add_systems(Update, spawn.run_if(on_timer(Duration::from_millis(200))))
        .init_resource::<Step>()
        .run();
}

fn setup(mut commands: Commands) {
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
        Transform::from_xyz(0.0, 0.0, -1.0),
    ));
}
