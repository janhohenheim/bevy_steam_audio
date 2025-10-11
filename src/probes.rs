use bevy_camera::primitives::Aabb;
use bevy_math::bounding::Aabb3d;

use crate::{prelude::*, scene::SteamAudioRootScene, wrapper::ToSteamAudioTransform};

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<SteamAudioProbeBatch>();
    app.add_observer(generate_probes);
}

#[derive(Event, Clone, Copy, Debug, PartialEq)]
pub struct GenerateProbes {
    pub spacing: f32,
    pub height: f32,
    pub aabb: Option<Aabb3d>,
}

impl Default for GenerateProbes {
    fn default() -> Self {
        Self {
            spacing: 2.0,
            height: 1.5,
            aabb: None,
        }
    }
}

#[derive(Resource, Debug, Deref, DerefMut)]
pub struct SteamAudioProbeBatch(audionimbus::ProbeBatch);

impl FromWorld for SteamAudioProbeBatch {
    fn from_world(_world: &mut World) -> Self {
        SteamAudioProbeBatch(audionimbus::ProbeBatch::try_new(&STEAM_AUDIO_CONTEXT).unwrap())
    }
}

fn generate_probes(
    generate: On<GenerateProbes>,
    aabbs: Query<&Aabb>,
    root: Res<SteamAudioRootScene>,
    mut batch: ResMut<SteamAudioProbeBatch>,
) -> Result {
    let aabb = if let Some(aabb) = generate.aabb {
        aabb
    } else {
        aabbs
            .iter()
            .fold(Aabb3d::new(Vec3A::ZERO, Vec3A::ZERO), |acc, aabb: &Aabb| {
                let min = acc.min.min(aabb.min());
                let max = acc.max.max(aabb.max());
                Aabb3d::new(min, max)
            })
    };
    let scale = aabb.max - aabb.min;
    let translation = aabb.min;
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
    batch.add_probe_array(&array);
    batch.commit();

    Ok(())
}
