use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use crate::{
    AMBISONICS_NUM_CHANNELS, AMBISONICS_ORDER, FRAME_SIZE, Listener,
    nodes::{decoder::AmbisonicDecodeNode, encoder::AudionimbusNode, reverb::ReverbDataNode},
    prelude::*,
};

use bevy_seedling::{context::StreamStartEvent, prelude::*};
use bevy_tasks::AsyncComputeTaskPool;
use bevy_transform::TransformSystems;
use firewheel::{diff::EventQueue, event::NodeEventType};

use crate::wrapper::*;

pub(super) fn plugin(app: &mut App) {
    app.add_systems(PreStartup, setup_audionimbus);

    app.add_systems(
        PostUpdate,
        (
            update_simulation.run_if(
                resource_exists::<AudionimbusSimulator>
                    .and(resource_exists::<ReflectAndPathingSimulationSynchronization>),
            ),
            prepare_seedling_data,
        )
            .chain()
            .after(TransformSystems::Propagate),
    );
    app.init_resource::<SteamAudioSettings>();
    app.add_observer(late_init);
}

pub(crate) fn setup_audionimbus(mut commands: Commands) {
    let context = audionimbus::Context::try_new(&audionimbus::ContextSettings::default()).unwrap();

    let ambisonic_node = AudionimbusNode::new(context.clone());
    let ambisonic_decode_node = AmbisonicDecodeNode::new(context.clone());

    commands
        .spawn((
            SamplerPool(SteamAudioPool),
            VolumeNode::default(),
            VolumeNodeConfig {
                channels: NonZeroChannelCount::new(AMBISONICS_NUM_CHANNELS).unwrap(),
            },
            sample_effects![ambisonic_node],
        ))
        // we only need one decoder
        .chain_node(ambisonic_decode_node);

    commands.insert_resource(AudionimbusContext(context));
}

#[derive(Debug, Clone, PartialEq, Reflect, Resource)]
#[reflect(Resource)]
struct SteamAudioSettings {
    pub enabled: bool,

    /// The number of rays to trace from the listener.
    /// Increasing this value results in more accurate reflections, at the cost of increased CPU usage.
    pub num_rays: u32,

    /// The number of times each ray traced from the listener is reflected when it encounters a solid object.
    /// Increasing this value results in longer, more accurate reverb tails, at the cost of increased CPU usage during simulation.
    pub num_bounces: u32,

    /// The duration (in seconds) of the impulse responses generated when simulating reflections.
    /// Increasing this value results in longer, more accurate reverb tails, at the cost of increased CPU usage during audio processing.
    pub duration: f32,

    /// The Ambisonic order of the impulse responses generated when simulating reflections.
    /// Increasing this value results in more accurate directional variation of reflected sound, at the cost of increased CPU usage during audio processing.
    pub order: u32,

    /// When calculating how much sound energy reaches a surface directly from a source, any source that is closer than [`Self::irradiance_min_distance`] to the surface is assumed to be at a distance of [`Self::irradiance_min_distance`], for the purposes of energy calculations.
    pub irradiance_min_distance: f32,

    pub reflection_and_pathing_simulation_timer: Option<Timer>,
}

impl SteamAudioSettings {
    pub fn to_audionimbus_simulation_shared_inputs(&self) -> audionimbus::SimulationSharedInputs {
        audionimbus::SimulationSharedInputs {
            num_rays: self.num_rays,
            num_bounces: self.num_bounces,
            duration: self.duration,
            order: self.order,
            irradiance_min_distance: self.irradiance_min_distance,
            listener: default(),
            pathing_visualization_callback: None,
        }
    }
}

impl Default for SteamAudioSettings {
    fn default() -> Self {
        Self {
            enabled: todo!(),
            num_rays: todo!(),
            num_bounces: todo!(),
            duration: todo!(),
            order: todo!(),
            irradiance_min_distance: todo!(),
            reflection_and_pathing_simulation_timer: todo!(),
        }
    }
}

/// Inspired by the Unity Steam Audio plugin.
fn update_simulation(
    simulator: Res<AudionimbusSimulator>,
    mut settings: ResMut<SteamAudioSettings>,
    listener: Single<&GlobalTransform, With<Listener>>,
    synchro: ResMut<ReflectAndPathingSimulationSynchronization>,
    time: Res<Time>,
) -> Result {
    if !settings.enabled {
        return Ok(());
    }
    let transform = listener.compute_transform();
    let shared_inputs = audionimbus::SimulationSharedInputs {
        listener: AudionimbusCoordinateSystem::from_bevy_transform(transform).to_audionimbus(),
        ..settings.to_audionimbus_simulation_shared_inputs()
    };

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

fn late_init(
    stream_start: On<StreamStartEvent>,
    mut commands: Commands,
    context: Res<AudionimbusContext>,
) {
    let sample_rate = stream_start.sample_rate;
    let mut simulator = audionimbus::Simulator::builder(
        audionimbus::SceneParams::Default,
        sample_rate.get(),
        FRAME_SIZE,
    )
    .with_direct(audionimbus::DirectSimulationSettings {
        max_num_occlusion_samples: 16,
    })
    .with_reflections(audionimbus::ReflectionsSimulationSettings::Convolution {
        max_num_rays: 2048,
        num_diffuse_samples: 8,
        max_duration: 2.0,
        max_order: AMBISONICS_ORDER,
        max_num_sources: 8,
        num_threads: 1,
    })
    .with_pathing(audionimbus::PathingSimulationSettings {
        num_visibility_samples: 32,
    })
    .try_build(&context)
    .unwrap();
    let listener_source = audionimbus::Source::try_new(
        &simulator,
        &audionimbus::SourceSettings {
            flags: audionimbus::SimulationFlags::REFLECTIONS,
        },
    )
    .unwrap();
    simulator.add_source(&listener_source);
    simulator.commit();
    commands.insert_resource(ListenerSource(listener_source));
    commands.insert_resource(AudionimbusSimulator(simulator.clone()));

    let simulation_complete = Arc::new(AtomicBool::new(false));
    let simulation_complete_inner = simulation_complete.clone();
    let (tx, rx) = crossbeam_channel::unbounded::<()>();
    let future = async move {
        loop {
            rx.recv().unwrap();
            simulator.run_reflections();
            simulation_complete_inner.store(true, Ordering::Relaxed);
        }
    };

    AsyncComputeTaskPool::get().spawn(future).detach();

    commands.insert_resource(ReflectAndPathingSimulationSynchronization {
        sender: tx,
        complete: simulation_complete,
    });

    commands.trigger(SimulatorReady);
}

#[derive(Resource, Deref, DerefMut)]
pub struct AudionimbusContext(pub(crate) audionimbus::Context);

#[derive(Resource, Deref, DerefMut)]
pub struct AudionimbusSimulator(
    pub audionimbus::Simulator<audionimbus::Direct, audionimbus::Reflections, audionimbus::Pathing>,
);

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
    camera: Single<&GlobalTransform, With<Listener>>,
    mut listener_source: ResMut<ListenerSource>,
) -> Result {
    let camera_transform = camera.into_inner().compute_transform();
    let listener_position = camera_transform.translation;
    let listener_orientation = AudionimbusCoordinateSystem::from_bevy_transform(camera_transform);

    // Listener source to simulate reverb.
    listener_source.set_inputs(
        audionimbus::SimulationFlags::REFLECTIONS,
        audionimbus::SimulationInputs {
            source: audionimbus::CoordinateSystem {
                origin: audionimbus::Vector3::new(
                    listener_position.x,
                    listener_position.y,
                    listener_position.z,
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
        node.listener_position = listener_position;
    }

    Ok(())
}
