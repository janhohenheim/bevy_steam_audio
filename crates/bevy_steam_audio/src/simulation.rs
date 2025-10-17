use std::{
    num::NonZeroU32,
    sync::{
        Arc, RwLock,
        atomic::{AtomicBool, Ordering},
    },
};

use crate::{
    STEAM_AUDIO_CONTEXT, SteamAudioListener,
    nodes::{
        SimulationOutputEvent, SteamAudioNodeConfig, SteamAudioReverbNodeConfig,
        encoder::SteamAudioNode, reverb::SteamAudioReverbNode,
    },
    prelude::*,
    probes::SteamAudioProbeBatch,
    scene::SteamAudioRootScene,
    settings::{SteamAudioEnabled, SteamAudioHrtf, SteamAudioPathingSettings, SteamAudioQuality},
    sources::{AudionimbusSource, ListenerSource, SourcesToRemove},
};

use bevy_seedling::{
    context::{StreamRestartEvent, StreamStartEvent},
    prelude::*,
};
use firewheel::{diff::EventQueue, event::NodeEventType};

use crate::wrapper::*;

pub(super) fn plugin(app: &mut App) {
    app.add_systems(
        PostUpdate,
        (recreate_simulator_on_settings_change)
            .in_set(SteamAudioSystems::CreateSimulator)
            .run_if(resource_exists::<AudionimbusSimulator>),
    );
    app.add_systems(
        PostUpdate,
        update_simulation
            .in_set(SteamAudioSystems::RunSimulator)
            .run_if(
                resource_exists::<AsyncSimulationSynchronization>
                    .and(resource_exists::<AudionimbusSimulator>),
            ),
    );
    app.add_observer(create_simulator)
        .add_observer(create_simulator_on_stream_start)
        .add_observer(create_simulator_on_stream_restart);
}

#[derive(Event)]
pub struct SteamAudioReady;

#[derive(Resource)]
struct AsyncSimulationSynchronization {
    sender: crossbeam_channel::Sender<()>,
    complete: Arc<AtomicBool>,
}

#[derive(Resource, Deref, DerefMut)]
pub struct AudionimbusSimulator {
    #[deref]
    pub simulator: Arc<
        RwLock<
            audionimbus::Simulator<
                audionimbus::Direct,
                audionimbus::Reflections,
                audionimbus::Pathing,
            >,
        >,
    >,
    pub sampling_rate: NonZeroU32,
}

#[derive(Event)]
struct CreateSimulator {
    sampling_rate: NonZeroU32,
}

fn create_simulator_on_stream_start(stream_start: On<StreamStartEvent>, mut commands: Commands) {
    commands.trigger(CreateSimulator {
        sampling_rate: stream_start.sample_rate,
    });
}

fn create_simulator_on_stream_restart(
    stream_restart: On<StreamRestartEvent>,
    mut commands: Commands,
) {
    commands.trigger(CreateSimulator {
        sampling_rate: stream_restart.current_rate,
    });
}

fn recreate_simulator_on_settings_change(
    quality: Res<SteamAudioQuality>,
    simulator: ResMut<AudionimbusSimulator>,
    mut commands: Commands,
    mut prev_quality: Local<Option<SteamAudioQuality>>,
) {
    let Some(prev_quality) = *prev_quality else {
        *prev_quality = Some(*quality);
        return;
    };

    if !quality.is_changed() && prev_quality == *quality {
        return;
    }

    commands.trigger(CreateSimulator {
        sampling_rate: simulator.sampling_rate,
    });
}

fn create_simulator(
    create: On<CreateSimulator>,
    mut commands: Commands,
    quality: Res<SteamAudioQuality>,
    root: ResMut<SteamAudioRootScene>,
    sources: Query<&AudionimbusSource>,
    probe_batch: Option<Res<SteamAudioProbeBatch>>,
    mut nodes: Query<&mut SteamAudioNodeConfig>,
    mut reverb_nodes: Query<&mut SteamAudioReverbNodeConfig>,
) -> Result {
    let settings = audionimbus::AudioSettings {
        sampling_rate: create.sampling_rate.into(),
        frame_size: quality.frame_size,
    };
    let hrtf = audionimbus::Hrtf::try_new(
        &STEAM_AUDIO_CONTEXT,
        &settings,
        &audionimbus::HrtfSettings {
            volume_normalization: audionimbus::VolumeNormalization::RootMeanSquared,
            ..default()
        },
    )
    .unwrap();
    for mut node_config in nodes.iter_mut() {
        *node_config = SteamAudioNodeConfig {
            quality: *quality,
            hrtf: Some(hrtf.clone()),
        }
    }
    for mut reverb_node_config in reverb_nodes.iter_mut() {
        *reverb_node_config = SteamAudioReverbNodeConfig {
            quality: *quality,
            hrtf: Some(hrtf.clone()),
        }
    }
    commands.insert_resource(SteamAudioHrtf(hrtf));
    // All sources to be removed are already removed by despawning the old simulator
    commands.insert_resource(SourcesToRemove::default());

    let mut simulator = audionimbus::Simulator::builder(
        audionimbus::SceneParams::Default,
        create.sampling_rate.into(),
        quality.frame_size,
    )
    .with_direct(quality.direct.into())
    .with_reflections(quality.reflections.to_audionimbus(quality.order))
    .with_pathing(quality.pathing.into())
    .try_build(&STEAM_AUDIO_CONTEXT)?;
    simulator.set_scene(&root);

    let listener_source = audionimbus::Source::try_new(
        &simulator,
        &audionimbus::SourceSettings {
            flags: audionimbus::SimulationFlags::REFLECTIONS,
        },
    )?;
    simulator.add_source(&listener_source);

    for source in &sources {
        simulator.add_source(source);
    }
    if let Some(probe_batch) = probe_batch {
        simulator.add_probe_batch(&probe_batch);
    }

    simulator.commit();

    let simulator = Arc::new(RwLock::new(simulator.clone()));
    commands.insert_resource(ListenerSource(listener_source));
    commands.insert_resource(AudionimbusSimulator {
        simulator: simulator.clone(),
        sampling_rate: create.sampling_rate,
    });

    let simulation_complete = Arc::new(AtomicBool::new(false));
    let simulation_complete_inner = simulation_complete.clone();
    let (tx, rx) = crossbeam_channel::unbounded::<()>();
    commands.insert_resource(AsyncSimulationSynchronization {
        sender: tx,
        complete: simulation_complete,
    });

    let future = async move {
        loop {
            {
                // Block thread until simulator is ready
                let simulator = simulator.read().unwrap();
                simulator.run_reflections();
                simulator.run_pathing();
            }

            simulation_complete_inner.store(true, Ordering::Relaxed);
            if rx.recv().is_err() {
                // tx dropped because we created a new simulation
                break;
            }
        }
    };
    AsyncComputeTaskPool::get().spawn(future).detach();

    commands.trigger(SteamAudioReady);
    Ok(())
}

/// Inspired by the Unity Steam Audio plugin.
fn update_simulation(
    simulator: ResMut<AudionimbusSimulator>,
    quality: Res<SteamAudioQuality>,
    mut enabled: ResMut<SteamAudioEnabled>,
    listener: Single<&GlobalTransform, With<SteamAudioListener>>,
    mut listener_source: ResMut<ListenerSource>,
    synchro: ResMut<AsyncSimulationSynchronization>,
    mut root: ResMut<SteamAudioRootScene>,
    mut nodes: Query<(
        &mut AudionimbusSource,
        &SteamAudioSamplePlayer,
        &GlobalTransform,
        &SampleEffects,
    )>,
    mut ambisonic_node: Query<(&mut SteamAudioNode, &mut AudioEvents)>,
    reverb_node: Single<(&mut SteamAudioReverbNode, &mut AudioEvents), Without<SteamAudioNode>>,

    pathing_settings: Res<SteamAudioPathingSettings>,
    probes: Option<Res<SteamAudioProbeBatch>>,
    time: Res<Time>,
) -> Result {
    if !enabled.enabled {
        return Ok(());
    }
    let listener_transform = listener.compute_transform();
    let listener_orientation = AudionimbusCoordinateSystem::from_bevy_transform(listener_transform);
    let shared_inputs = quality.to_audionimbus_simulation_shared_inputs(listener_orientation);

    if synchro.complete.load(Ordering::SeqCst) {
        root.commit();
        // This should never fail unless there's a bug, as this branch should only be called when the reflection thread is idle.
        simulator
            .try_write()
            .map_err(|e| format!("Failed to commit simulator even though it should be idle: {e}"))?
            .commit();
    }

    let listener_inputs = audionimbus::SimulationInputs {
        source: listener_orientation.to_audionimbus(),
        direct_simulation: None,
        reflections_simulation: Some(audionimbus::ReflectionsSimulationParameters::Convolution {
            baked_data_identifier: None,
        }),
        pathing_simulation: probes.as_ref().map(|probes| {
            audionimbus::PathingSimulationParameters {
                pathing_probes: probes,
                visibility_radius: pathing_settings.visibility_radius,
                visibility_threshold: pathing_settings.visibility_threshold,
                visibility_range: pathing_settings.visibility_range,
                pathing_order: quality.order,
                enable_validation: true,
                find_alternate_paths: true,
                deviation: audionimbus::DeviationModel::Default,
            }
        }),
    };
    // TODO: make this configurable
    let source_inputs = |orientation: AudionimbusCoordinateSystem| audionimbus::SimulationInputs {
        source: orientation.to_audionimbus(),
        direct_simulation: Some(audionimbus::DirectSimulationParameters {
            distance_attenuation: Some(audionimbus::DistanceAttenuationModel::Default),
            air_absorption: Some(audionimbus::AirAbsorptionModel::Default),
            directivity: Some(audionimbus::Directivity::default()),
            occlusion: Some(audionimbus::Occlusion {
                transmission: Some(audionimbus::TransmissionParameters {
                    num_transmission_rays: 4,
                }),
                algorithm: audionimbus::OcclusionAlgorithm::Raycast,
            }),
        }),
        reflections_simulation: Some(audionimbus::ReflectionsSimulationParameters::Convolution {
            baked_data_identifier: None,
        }),
        pathing_simulation: probes.as_ref().map(|probes| {
            audionimbus::PathingSimulationParameters {
                pathing_probes: probes,
                visibility_radius: pathing_settings.visibility_radius,
                visibility_threshold: pathing_settings.visibility_threshold,
                visibility_range: pathing_settings.visibility_range,
                pathing_order: quality.order,
                enable_validation: true,
                find_alternate_paths: true,
                deviation: audionimbus::DeviationModel::Default,
            }
        }),
    };

    let (mut reverb_node, mut reverb_events) = reverb_node.into_inner();
    // set inputs
    for (mut source, source_settings, transform, effects) in nodes.iter_mut() {
        if !source_settings
            .flags
            .contains(audionimbus::SimulationFlags::DIRECT)
        {
            continue;
        }
        let transform = transform.compute_transform();
        let orientation = AudionimbusCoordinateSystem::from_bevy_transform(transform);

        source.set_inputs(
            audionimbus::SimulationFlags::DIRECT,
            source_inputs(orientation),
        );

        let (mut node, mut _events) = ambisonic_node.get_effect_mut(effects)?;

        node.source_position = orientation;
        node.listener_position = listener_orientation;
    }

    listener_source.set_inputs(audionimbus::SimulationFlags::DIRECT, listener_inputs);
    reverb_node.listener_position = listener_orientation;

    let simulator = simulator
        .try_read()
        .map_err(|e| format!("Failed to run simulator even though it should be idle: {e}"))?;

    simulator.set_shared_inputs(audionimbus::SimulationFlags::DIRECT, &shared_inputs);

    simulator.run_direct();

    // read outputs
    for (mut source, source_settings, _transform, effects) in nodes.iter_mut() {
        if !source_settings
            .flags
            .contains(audionimbus::SimulationFlags::DIRECT)
        {
            continue;
        }
        let simulation_outputs = source.get_outputs(audionimbus::SimulationFlags::DIRECT);

        let (mut _node, mut events) = ambisonic_node.get_effect_mut(effects)?;
        events.push(NodeEventType::custom(SimulationOutputEvent {
            flags: audionimbus::SimulationFlags::DIRECT,
            outputs: simulation_outputs,
        }));
    }

    let Some(timer) = enabled.reflection_and_pathing_simulation_timer.as_mut() else {
        // User doesn't want any reflection or pathing simulation
        return Ok(());
    };
    timer.tick(time.delta());
    if !timer.is_finished() {
        // Not yet time to kick off expensive simulation
        return Ok(());
    }
    if !synchro.complete.load(Ordering::SeqCst) {
        // It's time, but the previous simulation is still running!
        return Ok(());
    }

    // The previous simulation is complete, so we can start the next one

    // Read the newest outputs
    for (mut source, source_settings, _transform, effects) in nodes.iter_mut() {
        let mut flags = source_settings.flags;
        flags.remove(audionimbus::SimulationFlags::DIRECT);
        if flags.is_empty() {
            continue;
        }
        let simulation_outputs = source.get_outputs(flags);

        let (mut _node, mut events) = ambisonic_node.get_effect_mut(effects)?;
        events.push(NodeEventType::custom(SimulationOutputEvent {
            flags,
            outputs: simulation_outputs,
        }));
    }

    let params: audionimbus::ReflectionEffectParams = listener_source
        .get_outputs(audionimbus::SimulationFlags::REFLECTIONS)
        .reflections()
        .into_inner();
    reverb_events.push(NodeEventType::custom(params));

    // set new inputs
    simulator.set_shared_inputs(
        audionimbus::SimulationFlags::REFLECTIONS | audionimbus::SimulationFlags::PATHING,
        &shared_inputs,
    );

    listener_source.set_inputs(
        audionimbus::SimulationFlags::REFLECTIONS | audionimbus::SimulationFlags::PATHING,
        listener_inputs,
    );

    for (mut source, source_settings, transform, effects) in nodes.iter_mut() {
        let mut flags = source_settings.flags;
        flags.remove(audionimbus::SimulationFlags::DIRECT);
        if flags.is_empty() {
            continue;
        }
        let transform = transform.compute_transform();
        let orientation = AudionimbusCoordinateSystem::from_bevy_transform(transform);

        source.set_inputs(flags, source_inputs(orientation));
        let (mut node, mut _events) = ambisonic_node.get_effect_mut(effects)?;
        node.pathing_available = probes.is_some();
    }

    synchro.complete.store(false, Ordering::SeqCst);
    timer.reset();
    synchro.sender.send(())?;

    Ok(())
}
