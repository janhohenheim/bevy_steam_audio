use std::{
    f32::consts::TAU,
    thread::sleep,
    time::{Duration, Instant},
};

use bevy::prelude::*;
use bevy_app::{PluginsState, ScheduleRunnerPlugin};
use bevy_mesh::MeshPlugin;
use bevy_seedling::prelude::*;
use bevy_steam_audio::{
    prelude::*,
    scene::mesh_backend::{Mesh3dSteamAudioScenePlugin, SteamAudioMesh},
};
use criterion::{Criterion, criterion_group, criterion_main};

const MAX_FRAMERATE: f32 = 200.0;

fn bevy_app(num_sources: usize) -> App {
    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins.set(ScheduleRunnerPlugin::run_loop(Duration::from_secs_f32(
            1.0 / MAX_FRAMERATE,
        ))),
        AssetPlugin::default(),
        MeshPlugin,
        TransformPlugin,
        SeedlingPlugin::default(),
        SteamAudioPlugin::default(),
        Mesh3dSteamAudioScenePlugin::default(),
    ))
    .insert_resource(SteamAudioQuality {
        reflections: SteamAudioReflectionsQuality {
            max_num_sources: num_sources as u32 + 1,
            ..default()
        },
        ..default()
    })
    .add_systems(
        Startup,
        move |mut commands: Commands,
              assets: Res<AssetServer>,
              mut meshes: ResMut<Assets<Mesh>>| {
            commands.spawn((
                Transform::default(),
                GlobalTransform::default(),
                SteamAudioListener,
            ));

            commands.spawn((
                Mesh3d(meshes.add(Cuboid::new(3.0, 2.0, 0.5))),
                Transform::from_xyz(0.0, 0.0, -4.0),
                SteamAudioMesh::default(),
            ));

            // Spawn sample players in a circle around the listener
            for i in 0..num_sources {
                let angle = i as f32 * TAU / num_sources as f32;
                let dist = 5.0;
                let x = angle.cos() * dist;
                let z = angle.sin() * dist;
                commands.spawn((
                    SamplePlayer::new(assets.load("selfless_courage.ogg")),
                    SteamAudioPool,
                    Transform::from_xyz(x, 0.0, z),
                ));
            }
        },
    );
    while app.plugins_state() == PluginsState::Adding {
        bevy::tasks::tick_global_task_pools_on_main_thread();
    }
    app.finish();
    app.cleanup();

    app
}

fn benchmarks(c: &mut Criterion) {
    let mut bench = |num_sources: usize| {
        let mut app = bevy_app(num_sources);
        let frame_time = Duration::from_secs_f64(1.0 / MAX_FRAMERATE as f64);
        let mut last = Instant::now();
        c.bench_function(&format!("{num_sources} source(s)"), |b| {
            b.iter(|| {
                app.update();
                let elapsed = last.elapsed();
                if elapsed < frame_time {
                    sleep(frame_time - elapsed);
                }
                last = Instant::now();
            });
        });
    };
    bench(1);
    bench(10);
    bench(50);
}

criterion_group!(benches, benchmarks);
criterion_main!(benches);
