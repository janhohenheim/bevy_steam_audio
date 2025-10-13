use bevy_camera::primitives::Aabb;
use bevy_math::bounding::{Aabb3d, BoundingVolume};

use crate::{
    prelude::*, scene::SteamAudioRootScene, simulation::AudionimbusSimulator,
    wrapper::ToSteamAudioTransform,
};

pub(super) fn plugin(app: &mut App) {
    app.add_systems(
        PostUpdate,
        generate_probes
            .in_set(SteamAudioSystems::GenerateProbes)
            // Important to have a run condition to not try to lock the simulator every frame
            .run_if(on_message::<GenerateProbes>),
    );
    app.add_message::<GenerateProbes>();
}

#[derive(Message, Clone, Copy, Debug, PartialEq)]
pub struct GenerateProbes {
    pub spacing: f32,
    pub height: f32,
    pub aabb: Option<Aabb3d>,
}

impl Default for GenerateProbes {
    fn default() -> Self {
        Self {
            spacing: 5.0,
            height: 1.5,
            aabb: None,
        }
    }
}

#[derive(Resource, Debug, Deref, DerefMut)]
pub struct SteamAudioProbeBatch(pub audionimbus::ProbeBatch);

fn generate_probes(
    mut generate_probes: ResMut<Messages<GenerateProbes>>,
    aabbs: Query<&Aabb>,
    root: Res<SteamAudioRootScene>,
    mut commands: Commands,
    simulator: Res<AudionimbusSimulator>,
    mut to_retry: Local<Vec<GenerateProbes>>,
) -> Result {
    let mut global_aabb = None;
    for generate in generate_probes.drain() {
        let Ok(mut simulator) = simulator.try_write() else {
            // Simulator is in use, try again next frame
            to_retry.push(generate);
            continue;
        };
        let aabb = if let Some(aabb) = generate.aabb {
            aabb
        } else {
            *global_aabb.get_or_insert_with(|| {
                aabbs
                    .iter()
                    .fold(Aabb3d::new(Vec3A::ZERO, Vec3A::ZERO), |acc, aabb: &Aabb| {
                        let min = acc.min.min(aabb.min());
                        let max = acc.max.max(aabb.max());
                        Aabb3d::new(min, max)
                    })
            })
        };
        let scale = aabb.max - aabb.min;
        let translation = aabb.center();
        let transform = GlobalTransform::from(
            Transform::from_translation(translation.into()).with_scale(scale.into()),
        )
        .to_steam_audio_transform();

        let params = audionimbus::ProbeGenerationParams::UniformFloor {
            spacing: generate.spacing,
            height: generate.height,
            transform,
        };
        let mut array = audionimbus::ProbeArray::try_new(&STEAM_AUDIO_CONTEXT)?;
        array.generate_probes(&root, &params);
        if array.num_probes() == 0 {
            error!("Failed to generate any probes. Is the scene empty?");
            return Ok(());
        }
        let mut batch = audionimbus::ProbeBatch::try_new(&STEAM_AUDIO_CONTEXT)?;
        batch.add_probe_array(&array);
        batch.commit();
        simulator.add_probe_batch(&batch);
        commands.insert_resource(SteamAudioProbeBatch(batch));
    }
    for generate in to_retry.drain(..) {
        generate_probes.write(generate);
    }
    Ok(())
}
