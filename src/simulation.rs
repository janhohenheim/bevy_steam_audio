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
        SteamAudioPool, decoder::AmbisonicDecodeNode, encoder::SteamAudioNode,
        reverb::ReverbDataNode,
    },
    prelude::*,
    scene::SteamAudioRootScene,
    settings::{SteamAudioQuality, SteamAudioSimulationSettings},
};

use bevy_seedling::{
    context::{StreamRestartEvent, StreamStartEvent},
    prelude::*,
};
use bevy_transform::TransformSystems;
use firewheel::{diff::EventQueue, event::NodeEventType};

use crate::wrapper::*;

pub(super) fn plugin(app: &mut App) {
    app.add_systems(
        PostUpdate,
        (
            create_simulator_on_settings_change,
            update_simulation.run_if(resource_exists::<AsyncSimulationSynchronization>),
        )
            .chain()
            .run_if(resource_exists::<AudionimbusSimulator>)
            .after(TransformSystems::Propagate),
    );
    app.init_resource::<SteamAudioSimulationSettings>()
        .init_resource::<SteamAudioQuality>();
    app.add_observer(create_simulator)
        .add_observer(create_simulator_on_stream_start)
        .add_observer(create_simulator_on_stream_restart);
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

fn create_simulator_on_settings_change(
    quality: Res<SteamAudioQuality>,
    simulator: Res<AudionimbusSimulator>,
    mut commands: Commands,
) {
    if quality.is_added() {
        return;
    }
    if quality.is_changed() {
        commands.trigger(CreateSimulator {
            sampling_rate: simulator.sampling_rate,
        });
    }
}

/// Inspired by the Unity Steam Audio plugin.
fn update_simulation(
    simulator: Res<AudionimbusSimulator>,
    quality: Res<SteamAudioQuality>,
    mut sim_settings: ResMut<SteamAudioSimulationSettings>,
    listener: Single<&GlobalTransform, With<SteamAudioListener>>,
    mut listener_source: ResMut<ListenerSource>,
    synchro: ResMut<AsyncSimulationSynchronization>,
    mut root: ResMut<SteamAudioRootScene>,
    mut nodes: Query<(&mut AudionimbusSource, &GlobalTransform, &SampleEffects)>,
    mut ambisonic_node: Query<(&mut SteamAudioNode, &mut AudioEvents)>,
    mut decode_node: Single<&mut AmbisonicDecodeNode>,
    mut reverb_data: Single<&mut AudioEvents, (With<ReverbDataNode>, Without<SteamAudioNode>)>,
    time: Res<Time>,
) -> Result {
    if !sim_settings.enabled {
        return Ok(());
    }
    let listener_transform = listener.compute_transform();
    let listener_orientation = AudionimbusCoordinateSystem::from_bevy_transform(listener_transform);
    let shared_inputs =
        sim_settings.to_audionimbus_simulation_shared_inputs(listener_orientation, *quality);

    if synchro.complete.load(Ordering::SeqCst) {
        // TODO: only do this if necessary
        root.commit();
        {
            let mut simulator = simulator.write().unwrap();
            simulator.set_scene(&root);
            simulator.commit();
        }
    }
    {
        let simulator = simulator.read().unwrap();
        simulator.set_shared_inputs(audionimbus::SimulationFlags::DIRECT, &shared_inputs);
        decode_node.listener_orientation = listener_orientation;

        for (mut source, transform, _effects) in nodes.iter_mut() {
            let orientation =
                AudionimbusCoordinateSystem::from_bevy_transform(transform.compute_transform());

            source.set_inputs(
                audionimbus::SimulationFlags::DIRECT,
                audionimbus::SimulationInputs {
                    source: orientation.to_audionimbus(),
                    direct_simulation: Some(audionimbus::DirectSimulationParameters {
                        distance_attenuation: Some(audionimbus::DistanceAttenuationModel::Default),
                        air_absorption: Some(audionimbus::AirAbsorptionModel::Default),
                        directivity: Some(audionimbus::Directivity::default()),
                        occlusion: Some(audionimbus::Occlusion {
                            transmission: Some(audionimbus::TransmissionParameters {
                                num_transmission_rays: 8,
                            }),
                            algorithm: audionimbus::OcclusionAlgorithm::Raycast,
                        }),
                    }),
                    reflections_simulation: Some(
                        audionimbus::ReflectionsSimulationParameters::Convolution {
                            baked_data_identifier: None,
                        },
                    ),
                    pathing_simulation: None,
                },
            );
        }

        simulator.run_direct();

        for (mut source, transform, effects) in nodes.iter_mut() {
            let transform = transform.compute_transform();
            let source_position = transform.translation;

            let simulation_outputs = source.get_outputs(audionimbus::SimulationFlags::DIRECT);

            let (mut node, mut events) = ambisonic_node.get_effect_mut(effects)?;
            events.push(NodeEventType::custom(SimulationOutputEvent(
                simulation_outputs,
            )));
            node.source_position = source_position;
            node.listener_position = listener_transform.translation;
        }

        let Some(timer) = sim_settings
            .reflection_and_pathing_simulation_timer
            .as_mut()
        else {
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
        synchro.complete.store(false, Ordering::SeqCst);
        timer.reset();

        // Read the newest outputs
        let reverb_simulation_outputs = listener_source.get_outputs(
            audionimbus::SimulationFlags::REFLECTIONS | audionimbus::SimulationFlags::PATHING,
        );

        reverb_data.push(NodeEventType::custom(
            reverb_simulation_outputs.reflections().into_inner(),
        ));

        // set new inputs

        simulator.set_shared_inputs(
            audionimbus::SimulationFlags::REFLECTIONS | audionimbus::SimulationFlags::PATHING,
            &shared_inputs,
        );
        // Listener source to simulate reverb.
        listener_source.set_inputs(
            audionimbus::SimulationFlags::REFLECTIONS,
            audionimbus::SimulationInputs {
                source: listener_orientation.to_audionimbus(),
                direct_simulation: Some(audionimbus::DirectSimulationParameters {
                    distance_attenuation: Some(audionimbus::DistanceAttenuationModel::Default),
                    air_absorption: Some(audionimbus::AirAbsorptionModel::Default),
                    directivity: Some(audionimbus::Directivity::default()),
                    occlusion: Some(audionimbus::Occlusion {
                        transmission: Some(audionimbus::TransmissionParameters {
                            num_transmission_rays: 8,
                        }),
                        algorithm: audionimbus::OcclusionAlgorithm::Raycast,
                    }),
                }),
                reflections_simulation: Some(
                    audionimbus::ReflectionsSimulationParameters::Convolution {
                        baked_data_identifier: None,
                    },
                ),
                pathing_simulation: None,
            },
        );
    }
    synchro.sender.send(())?;

    Ok(())
}

fn temp(
    mut nodes: Query<(&mut AudionimbusSource, &GlobalTransform, &SampleEffects)>,
    mut ambisonic_node: Query<(&mut SteamAudioNode, &mut AudioEvents)>,
    mut decode_node: Single<&mut AmbisonicDecodeNode>,
    mut reverb_data: Single<&mut AudioEvents, (With<ReverbDataNode>, Without<SteamAudioNode>)>,
    listener: Single<&GlobalTransform, With<SteamAudioListener>>,
    mut listener_source: ResMut<ListenerSource>,
) {
}

#[derive(Event)]
pub(crate) struct SimulatorReady;

#[derive(Resource)]
struct AsyncSimulationSynchronization {
    sender: crossbeam_channel::Sender<()>,
    complete: Arc<AtomicBool>,
}

fn create_simulator(
    create: On<CreateSimulator>,
    mut commands: Commands,
    quality: Res<SteamAudioQuality>,
) -> Result {
    let mut simulator = audionimbus::Simulator::builder(
        audionimbus::SceneParams::Default,
        create.sampling_rate.into(),
        quality.frame_size,
    )
    .with_direct(quality.direct.into())
    .with_reflections(quality.reflections.to_audionimbus(quality.order))
    .with_pathing(quality.pathing.into())
    .try_build(&STEAM_AUDIO_CONTEXT)?;

    let listener_source = audionimbus::Source::try_new(
        &simulator,
        &audionimbus::SourceSettings {
            flags: audionimbus::SimulationFlags::REFLECTIONS,
        },
    )?;
    simulator.add_source(&listener_source);
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
            if rx.recv().is_err() {
                // tx dropped because we created a new simulation
                break;
            }
            simulator.read().unwrap().run_reflections();
            simulation_complete_inner.store(true, Ordering::Relaxed);
        }
    };
    AsyncComputeTaskPool::get().spawn(future).detach();

    commands.trigger(SimulatorReady);
    Ok(())
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

#[derive(Resource, Deref, DerefMut)]
pub(crate) struct ListenerSource(pub(crate) audionimbus::Source);

// TODO: Add an API for `SteamAudioSamplePlayer` that includes all source-related settings.
// When that component changes, also update the configs on the nodes.
// Do the nodes also get rebuilt when the sampling rate changes? If not, also rebuild them then.

#[derive(Component, Deref, DerefMut)]
#[require(Transform, GlobalTransform, SteamAudioPool)]
pub(crate) struct AudionimbusSource(pub(crate) audionimbus::Source);

pub(crate) struct SimulationOutputEvent(pub(crate) audionimbus::SimulationOutputs);
