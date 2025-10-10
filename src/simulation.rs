use std::{
    num::NonZeroU32,
    sync::{
        Arc, RwLock,
        atomic::{AtomicBool, Ordering},
    },
};

use crate::{
    FRAME_SIZE, Listener,
    nodes::{decoder::AmbisonicDecodeNode, encoder::AudionimbusNode, reverb::ReverbDataNode},
    prelude::*,
    settings::{SteamAudioSettings, SteamAudioSimulatorSettings},
};

use bevy_seedling::{
    context::{StreamRestartEvent, StreamStartEvent},
    prelude::*,
};
use bevy_transform::TransformSystems;
use firewheel::{diff::EventQueue, event::NodeEventType};

use crate::wrapper::*;

pub(super) fn plugin(app: &mut App) {
    app.add_systems(PreStartup, setup_audionimbus);

    app.add_systems(
        PostUpdate,
        (
            create_simulator_on_settings_change,
            update_simulation.run_if(resource_exists::<ReflectAndPathingSimulationSynchronization>),
            prepare_seedling_data,
        )
            .chain()
            .run_if(resource_exists::<AudionimbusSimulator>)
            .after(TransformSystems::Propagate),
    );
    app.init_resource::<SteamAudioSettings>()
        .init_resource::<SteamAudioSimulatorSettings>();
    app.add_observer(create_simulator)
        .add_observer(create_simulator_on_stream_start)
        .add_observer(create_simulator_on_stream_restart);
}

pub(crate) fn setup_audionimbus(
    mut commands: Commands,
    settings: Res<SteamAudioSimulatorSettings>,
) {
    let context = audionimbus::Context::try_new(&audionimbus::ContextSettings::default()).unwrap();

    let ambisonic_node = AudionimbusNode::new(context.clone());
    let ambisonic_decode_node = AmbisonicDecodeNode::new(context.clone());

    commands
        .spawn((
            SamplerPool(SteamAudioPool),
            VolumeNode::default(),
            VolumeNodeConfig {
                channels: NonZeroChannelCount::new(settings.num_channels()).unwrap(),
            },
            sample_effects![ambisonic_node],
        ))
        // we only need one decoder
        .chain_node(ambisonic_decode_node);

    commands.insert_resource(AudionimbusContext(context));
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
    settings: Res<SteamAudioSimulatorSettings>,
    simulator: Res<AudionimbusSimulator>,
    mut commands: Commands,
) {
    if settings.is_changed() {
        commands.trigger(CreateSimulator {
            sampling_rate: simulator.sampling_rate,
        });
    }
}

/// Inspired by the Unity Steam Audio plugin.
fn update_simulation(
    simulator: Res<AudionimbusSimulator>,
    simulator_settings: Res<SteamAudioSimulatorSettings>,
    mut settings: ResMut<SteamAudioSettings>,
    listener: Single<&GlobalTransform, With<Listener>>,
    synchro: ResMut<ReflectAndPathingSimulationSynchronization>,
    time: Res<Time>,
) -> Result {
    if !settings.enabled {
        return Ok(());
    }
    let transform = listener.compute_transform();
    let shared_inputs = settings.to_audionimbus_simulation_shared_inputs(
        AudionimbusCoordinateSystem::from_bevy_transform(transform),
        simulator_settings.clone(),
    );

    {
        let simulator = simulator.read().unwrap();
        simulator.set_shared_inputs(audionimbus::SimulationFlags::DIRECT, &shared_inputs);
        simulator.run_direct();

        let Some(timer) = settings.reflection_and_pathing_simulation_timer.as_mut() else {
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

        simulator.set_shared_inputs(
            audionimbus::SimulationFlags::REFLECTIONS | audionimbus::SimulationFlags::PATHING,
            &shared_inputs,
        );
    }
    synchro.sender.send(())?;

    Ok(())
}

#[derive(PoolLabel, PartialEq, Eq, Debug, Hash, Clone, Default)]
pub(crate) struct SteamAudioPool;

#[derive(Event)]
pub(crate) struct SimulatorReady;

#[derive(Resource)]
struct ReflectAndPathingSimulationSynchronization {
    sender: crossbeam_channel::Sender<()>,
    complete: Arc<AtomicBool>,
}

fn create_simulator(
    create: On<CreateSimulator>,
    mut commands: Commands,
    context: Res<AudionimbusContext>,
    settings: Res<SteamAudioSimulatorSettings>,
) -> Result {
    let mut simulator = audionimbus::Simulator::builder(
        audionimbus::SceneParams::Default,
        create.sampling_rate.into(),
        FRAME_SIZE,
    )
    .with_direct(settings.direct.into())
    .with_reflections(settings.reflections.to_audionimbus(settings.order))
    .with_pathing(settings.pathing.into())
    .try_build(&context)?;

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
    let future = async move {
        loop {
            rx.recv().unwrap();
            simulator.read().unwrap().run_reflections();
            simulation_complete_inner.store(true, Ordering::Relaxed);
        }
    };

    AsyncComputeTaskPool::get().spawn(future).detach();

    commands.insert_resource(ReflectAndPathingSimulationSynchronization {
        sender: tx,
        complete: simulation_complete,
    });

    commands.trigger(SimulatorReady);
    Ok(())
}

#[derive(Resource, Deref, DerefMut)]
pub struct AudionimbusContext(pub(crate) audionimbus::Context);

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

#[derive(Component, Deref, DerefMut)]
#[require(Transform, GlobalTransform, SteamAudioPool)]
pub(crate) struct AudionimbusSource(pub(crate) audionimbus::Source);

pub(crate) struct SimulationOutputEvent(pub(crate) audionimbus::SimulationOutputs);

fn prepare_seedling_data(
    mut nodes: Query<(&mut AudionimbusSource, &GlobalTransform, &SampleEffects)>,
    mut ambisonic_node: Query<(&mut AudionimbusNode, &mut AudioEvents)>,
    mut decode_node: Single<&mut AmbisonicDecodeNode>,
    mut reverb_data: Single<&mut AudioEvents, (With<ReverbDataNode>, Without<AudionimbusNode>)>,
    listener: Single<&GlobalTransform, With<Listener>>,
    mut listener_source: ResMut<ListenerSource>,
) -> Result {
    let listener_transform = listener.into_inner().compute_transform();
    let listener_orientation = AudionimbusCoordinateSystem::from_bevy_transform(listener_transform);

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

    let simulation_flags =
        audionimbus::SimulationFlags::DIRECT | audionimbus::SimulationFlags::REFLECTIONS;

    let reverb_simulation_outputs =
        listener_source.get_outputs(audionimbus::SimulationFlags::REFLECTIONS);

    reverb_data.push(NodeEventType::custom(
        reverb_simulation_outputs.reflections().into_inner(),
    ));

    decode_node.listener_orientation = listener_orientation;

    for (mut source, transform, effects) in nodes.iter_mut() {
        let transform = transform.compute_transform();
        let source_position = transform.translation;

        source.set_inputs(
            simulation_flags,
            audionimbus::SimulationInputs {
                source: audionimbus::CoordinateSystem {
                    origin: audionimbus::Vector3::new(
                        source_position.x,
                        source_position.y,
                        source_position.z,
                    ),
                    ..default()
                },
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

        let simulation_outputs = source.get_outputs(simulation_flags);

        let (mut node, mut events) = ambisonic_node.get_effect_mut(effects)?;
        events.push(NodeEventType::custom(SimulationOutputEvent(
            simulation_outputs,
        )));
        node.source_position = source_position;
        node.listener_position = listener_transform.translation;
    }

    Ok(())
}
