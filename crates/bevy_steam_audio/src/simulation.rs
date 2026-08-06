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
        SteamAudioNodeConfig, SteamAudioReverbNodeConfig, encoder::SteamAudioNode,
        reverb::SteamAudioReverbNode,
    },
    prelude::*,
    probes::SteamAudioProbeBatch,
    scene::SteamAudioRootScene,
    settings::{
        SteamAudioEnabled, SteamAudioHrtf, SteamAudioPathBakingSettings, SteamAudioQuality,
    },
    sources::{AudionimbusSource, ListenerSource, ListenerSourceInner, SourcesToRemove},
};

use bevy_seedling::{
    context::{StreamRestartEvent, StreamStartEvent},
    prelude::*,
};

use crate::wrapper::*;

pub(super) fn plugin(app: &mut App) {
    app.add_systems(
        PostUpdate,
        recreate_simulator_on_settings_change
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

type InnerSimulator = audionimbus::Simulator<
    'static,
    audionimbus::DefaultRayTracer,
    audionimbus::Direct,
    audionimbus::Reflections,
    audionimbus::Pathing,
>;

#[derive(Resource)]
pub struct AudionimbusSimulator {
    simulator: Arc<RwLock<InnerSimulator>>,
    pub sampling_rate: NonZeroU32,
    /// A permanently empty probe batch used as a placeholder for `set_inputs` calls when no real
    /// probes have been generated yet.
    fallback_probe_batch: audionimbus::ProbeBatch,
}

impl AudionimbusSimulator {
    /// Used to force consumers to only ever use `ResMut` and not `Res`,
    /// as running two things simultaneously on the underlying Steam Audio simulator
    /// needs to be carefully managed, even when using `.read()`. E.g. it's easy to accidentally
    /// have two systems adding a source to the same simulator in parallel if this was used with `Res`.
    pub fn get(&mut self) -> &Arc<RwLock<InnerSimulator>> {
        &self.simulator
    }
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
    let Some(prev_quality_value) = *prev_quality else {
        *prev_quality = Some(*quality);
        return;
    };

    if !quality.is_changed() && prev_quality_value == *quality {
        return;
    }

    *prev_quality = Some(*quality);

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
        };
    }
    for mut reverb_node_config in reverb_nodes.iter_mut() {
        *reverb_node_config = SteamAudioReverbNodeConfig {
            quality: *quality,
            hrtf: Some(hrtf.clone()),
        };
    }
    commands.insert_resource(SteamAudioHrtf(hrtf));
    // All sources to be removed are already removed by despawning the old simulator
    commands.insert_resource(SourcesToRemove::default());

    let simulator_settings = audionimbus::SimulationSettings::new(
        create.sampling_rate.into(),
        quality.frame_size,
        quality.order,
    )
    .with_direct(quality.direct.into())
    .with_reflections(quality.reflections.to_audionimbus())
    .with_pathing(quality.pathing.into());

    let mut simulator = audionimbus::Simulator::try_new(&STEAM_AUDIO_CONTEXT, &simulator_settings)?;
    simulator.set_scene(&root.0);

    let listener_source: ListenerSourceInner = audionimbus::Source::try_new(
        &simulator,
        &audionimbus::SourceSettings {
            flags: audionimbus::SimulationFlags::REFLECTIONS
                | audionimbus::SimulationFlags::PATHING,
        },
    )?;
    simulator.add_source(&listener_source);

    for source in &sources {
        simulator.add_source(&source.0);
    }
    if let Some(probe_batch) = probe_batch {
        simulator.add_probe_batch(&probe_batch.0);
    }

    simulator.commit();

    // Empty fallback batch
    let fallback_probe_batch = audionimbus::ProbeBatch::try_new(&STEAM_AUDIO_CONTEXT)?;

    let simulator_arc = Arc::new(RwLock::new(simulator));
    commands.insert_resource(ListenerSource(listener_source));
    commands.insert_resource(AudionimbusSimulator {
        simulator: simulator_arc.clone(),
        sampling_rate: create.sampling_rate,
        fallback_probe_batch,
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
                let simulator = simulator_arc.read().unwrap();
                let _ = simulator.run_reflections();
                let _ = simulator.run_pathing();
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

/// Builds the direct simulation parameters used for every audio source.
fn source_direct_params(quality: &SteamAudioQuality) -> audionimbus::DirectSimulationParameters {
    audionimbus::DirectSimulationParameters::new()
        .with_distance_attenuation(audionimbus::DistanceAttenuationModel::Default)
        .with_air_absorption(audionimbus::AirAbsorptionModel::Default)
        .with_directivity(audionimbus::Directivity::WeightedDipole {
            // TODO: synchronize with the encoder node
            weight: 0.0,
            power: 0.0,
        })
        .with_occlusion(
            audionimbus::Occlusion::new(audionimbus::OcclusionAlgorithm::Volumetric {
                radius: 0.3,
                num_occlusion_samples: quality.direct.max_num_occlusion_samples,
            })
            .with_transmission(audionimbus::TransmissionParameters {
                num_transmission_rays: 16,
            }),
        )
}

/// Builds the pathing parameters for a source, borrowing `probe_batch` for the call duration.
fn source_pathing_params<'a>(
    probe_batch: &'a audionimbus::ProbeBatch,
    pathing_settings: &SteamAudioPathBakingSettings,
    quality: &SteamAudioQuality,
) -> audionimbus::PathingSimulationParameters<'a> {
    audionimbus::PathingSimulationParameters {
        pathing_probes: probe_batch,
        visibility_radius: pathing_settings.visibility_radius,
        visibility_threshold: pathing_settings.visibility_threshold,
        visibility_range: pathing_settings.visibility_range,
        pathing_order: quality.order,
        enable_validation: true,
        find_alternate_paths: true,
        deviation: audionimbus::DeviationModel::Default,
    }
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
    mut nodes: Query<(&mut AudionimbusSource, &GlobalTransform, &SampleEffects)>,
    mut steam_audio_nodes: Query<&mut SteamAudioNode>,
    mut reverb_node: Single<&mut SteamAudioReverbNode, Without<EffectOf>>,
    pathing_settings: Res<SteamAudioPathBakingSettings>,
    probes: Option<Res<SteamAudioProbeBatch>>,
    time: Res<Time>,
    mut errors: Local<Vec<String>>,
) -> Result {
    if !enabled.enabled {
        return Ok(());
    }
    errors.clear();
    let listener_transform = listener.compute_transform();
    let listener_orientation: AudionimbusCoordinateSystem = listener_transform.into();
    let shared_inputs = quality.to_audionimbus_simulation_shared_inputs(listener_orientation);

    let simulator_arc = simulator.simulator.clone();
    let pathing_available = probes.is_some();
    let probe_batch_ref: &audionimbus::ProbeBatch = match probes.as_ref() {
        Some(p) => &p.0,
        None => &simulator.fallback_probe_batch,
    };

    if synchro.complete.load(Ordering::SeqCst) {
        root.0.commit();
        // This should never fail unless there's a bug, as this branch should only be called when the reflection thread is idle.
        simulator_arc
            .try_write()
            .map_err(|e| format!("Failed to commit simulator even though it should be idle: {e}"))?
            .commit();
    }

    let reflections_params = audionimbus::ReflectionsSimulationParameters::Convolution {
        baked_data_identifier: None,
    };

    // Per-source inputs

    for (mut source, transform, effects) in nodes.iter_mut() {
        let orientation: AudionimbusCoordinateSystem = transform.compute_transform().into();

        let inputs = audionimbus::SimulationInputs::new(orientation.into())
            .with_direct(source_direct_params(&quality))
            .with_reflections(reflections_params)
            .with_pathing(source_pathing_params(
                probe_batch_ref,
                &pathing_settings,
                &quality,
            ));

        if let Err(e) = source
            .0
            .set_inputs(audionimbus::SimulationFlags::DIRECT, inputs)
        {
            errors.push(format!("Failed to set source direct inputs: {e}"));
            continue;
        }

        let mut node = match steam_audio_nodes.get_effect_mut(effects) {
            Ok(node) => node,
            Err(err) => {
                errors.push(format!("Failed to get Steam Audio node from source: {err}"));
                continue;
            }
        };
        node.source_position = orientation;
        node.listener_position = listener_orientation;
    }

    {
        let inputs = audionimbus::SimulationInputs::new(listener_orientation.into())
            .with_direct(source_direct_params(&quality))
            .with_reflections(reflections_params)
            .with_pathing(source_pathing_params(
                probe_batch_ref,
                &pathing_settings,
                &quality,
            ));

        if let Err(e) = listener_source.0.set_inputs(
            audionimbus::SimulationFlags::REFLECTIONS | audionimbus::SimulationFlags::PATHING,
            inputs,
        ) {
            errors.push(format!("Failed to set listener source inputs: {e}"));
        }
    }

    reverb_node.listener_position = listener_orientation;

    let simulator_read = simulator_arc
        .try_read()
        .map_err(|e| format!("Failed to run simulator even though it should be idle: {e}"))?;

    if let Err(e) =
        simulator_read.set_shared_inputs(audionimbus::SimulationFlags::DIRECT, &shared_inputs)
    {
        errors.push(format!("Failed to set shared direct inputs: {e}"));
    }

    simulator_read.run_direct();

    let Some(timer) = enabled.reflection_and_pathing_simulation_timer.as_mut() else {
        // User doesn't want any reflection or pathing simulation
        if errors.is_empty() {
            return Ok(());
        }
        return Err(errors.join("\n").into());
    };
    timer.tick(time.delta());
    if !timer.is_finished() {
        // Not yet time to kick off expensive simulation
        if errors.is_empty() {
            return Ok(());
        }
        return Err(errors.join("\n").into());
    }
    if !synchro.complete.load(Ordering::SeqCst) {
        // It's time, but the previous simulation is still running!
        if errors.is_empty() {
            return Ok(());
        }
        return Err(errors.join("\n").into());
    }

    // The previous simulation is complete, so we can start the next one

    // set new inputs
    if let Err(e) = simulator_read.set_shared_inputs(
        audionimbus::SimulationFlags::REFLECTIONS | audionimbus::SimulationFlags::PATHING,
        &shared_inputs,
    ) {
        errors.push(format!(
            "Failed to set shared reflections/pathing inputs: {e}"
        ));
    }

    {
        let inputs = audionimbus::SimulationInputs::new(listener_orientation.into())
            .with_direct(source_direct_params(&quality))
            .with_reflections(reflections_params)
            .with_pathing(source_pathing_params(
                probe_batch_ref,
                &pathing_settings,
                &quality,
            ));

        if let Err(e) = listener_source.0.set_inputs(
            audionimbus::SimulationFlags::REFLECTIONS | audionimbus::SimulationFlags::PATHING,
            inputs,
        ) {
            errors.push(format!(
                "Failed to set listener reflections/pathing inputs: {e}"
            ));
        }
    }

    for (mut source, transform, effects) in nodes.iter_mut() {
        let orientation: AudionimbusCoordinateSystem = transform.compute_transform().into();

        let inputs = audionimbus::SimulationInputs::new(orientation.into())
            .with_direct(source_direct_params(&quality))
            .with_reflections(reflections_params)
            .with_pathing(source_pathing_params(
                probe_batch_ref,
                &pathing_settings,
                &quality,
            ));

        if let Err(e) = source.0.set_inputs(
            audionimbus::SimulationFlags::REFLECTIONS | audionimbus::SimulationFlags::PATHING,
            inputs,
        ) {
            errors.push(format!(
                "Failed to set source reflections/pathing inputs: {e}"
            ));
            continue;
        }

        let mut node = match steam_audio_nodes.get_effect_mut(effects) {
            Ok(node) => node,
            Err(err) => {
                errors.push(format!("Failed to get Steam Audio node from source: {err}"));
                continue;
            }
        };
        node.pathing_available = pathing_available;
    }

    synchro.complete.store(false, Ordering::SeqCst);
    timer.reset();
    synchro.sender.send(())?;

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("\n").into())
    }
}
