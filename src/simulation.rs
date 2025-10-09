use crate::{
    AMBISONICS_NUM_CHANNELS, AMBISONICS_ORDER, FRAME_SIZE, Listener,
    decoder::AmbisonicDecodeNode,
    encoder::{AudionimbusNode, SimulationUpdate},
    prelude::*,
};

use bevy_seedling::{context::StreamStartEvent, prelude::*};
use bevy_transform::TransformSystems;
use firewheel::{
    collector::{ArcGc, OwnedGc},
    diff::EventQueue,
    event::NodeEventType,
};

use crate::wrapper::*;

pub(super) fn plugin(app: &mut App) {
    app.add_systems(PreStartup, setup_audionimbus);

    app.add_systems(
        PostUpdate,
        prepare_seedling_data.after(TransformSystems::Propagate),
    );
    app.add_observer(late_init);
}

pub(crate) fn setup_audionimbus(mut commands: Commands) {
    let context = audionimbus::Context::try_new(&audionimbus::ContextSettings::default()).unwrap();

    commands.insert_resource(AudionimbusContext(context));
}

#[derive(PoolLabel, PartialEq, Eq, Debug, Hash, Clone, Default)]
pub(crate) struct SteamAudioPool;

#[derive(Event)]
pub(crate) struct AudionimbusReady;

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
    commands.insert_resource(AudionimbusSimulator(simulator));

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

    commands.trigger(AudionimbusReady);
}

#[derive(Resource, Deref, DerefMut)]
pub(crate) struct AudionimbusContext(pub(crate) audionimbus::Context);

#[derive(Resource, Deref, DerefMut)]
pub(crate) struct AudionimbusSimulator(
    pub(crate) audionimbus::Simulator<audionimbus::Direct, audionimbus::Reflections>,
);

#[derive(Resource, Deref, DerefMut)]
pub(crate) struct ListenerSource(pub(crate) audionimbus::Source);

#[derive(Component, Deref, DerefMut)]
#[require(Transform, GlobalTransform, SteamAudioPool)]
pub(crate) struct AudionimbusSource(pub(crate) audionimbus::Source);

fn prepare_seedling_data(
    mut nodes: Query<(&mut AudionimbusSource, &GlobalTransform, &SampleEffects)>,
    mut ambisonic_node: Query<(&mut AudionimbusNode, &mut AudioEvents)>,
    mut decode_node: Single<&mut AmbisonicDecodeNode>,
    camera: Single<&GlobalTransform, With<Listener>>,
    mut listener_source: ResMut<ListenerSource>,
    mut simulator: ResMut<AudionimbusSimulator>,
) -> Result {
    let camera_transform = camera.into_inner().compute_transform();
    let listener_position = camera_transform.translation;
    let listener_orientation = audionimbus::CoordinateSystem::from_transform(camera_transform);

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
    simulator.set_shared_inputs(
        simulation_flags,
        &audionimbus::SimulationSharedInputs {
            listener: listener_orientation,
            num_rays: 2048,
            num_bounces: 8,
            duration: 2.0,
            order: AMBISONICS_ORDER,
            irradiance_min_distance: 1.0,
            pathing_visualization_callback: None,
        },
    );
    simulator.run_direct();
    simulator.run_reflections();

    let reverb_simulation_outputs =
        listener_source.get_outputs(audionimbus::SimulationFlags::REFLECTIONS);
    let reverb_effect_params = ArcGc::new(OwnedGc::new(
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
        events.push(NodeEventType::Custom(OwnedGc::new(Box::new(
            SimulationUpdate {
                outputs: Some(simulation_outputs),
                reverb_effect_params: reverb_effect_params.clone(),
            },
        ))));
        node.source_position = source_position;
        node.listener_position = listener_position;
    }

    Ok(())
}
