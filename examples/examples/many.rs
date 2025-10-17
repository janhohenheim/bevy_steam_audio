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
        .insert_resource(SteamAudioQuality {
            reflections: SteamAudioReflectionsQuality {
                // Default value
                impulse_duration: Duration::from_millis(2000),
                // Slightly bumped to play more CAWs at the same time
                max_num_sources: 12,
                ..default()
            },
            ..default()
        })
        .add_systems(Startup, setup)
        .add_systems(
            Update,
            // Your bug goes here!
            bug_0.run_if(on_timer(Duration::from_millis(200))),
        )
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
        // Try also step1.ogg (extremely short audio, also crashes) and selfless_courage.ogg (long audio, never crashes)
        Self(assets.load("caw.ogg"))
    }
}

/// crash
fn bug_0(mut commands: Commands, step: Res<Step>) {
    commands.spawn((SamplePlayer::new(step.0.clone()), SteamAudioPool));
}

/// crash after a while
fn bug_1(mut commands: Commands, step: Res<Step>) {
    commands.spawn((SamplePlayer::new(step.0.clone()).looping(), SteamAudioPool));
}

/// No crash
fn working(mut commands: Commands, step: Res<Step>, mut times: Local<u32>) {
    if *times >= 11 {
        // 1 = max samples (12) - 1 for the listener
        return;
    }
    *times += 1;
    commands.spawn((SamplePlayer::new(step.0.clone()).looping(), SteamAudioPool));
}
