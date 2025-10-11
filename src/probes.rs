use bevy_camera::primitives::Aabb;
use bevy_math::bounding::Aabb3d;

use crate::{prelude::*, scene::SteamAudioRootScene, wrapper::ToSteamAudioTransform};

pub(super) fn plugin(app: &mut App) {
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
pub struct SteamAudioProbeBatch(pub audionimbus::ProbeBatch);

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

    let id = audionimbus::BakedDataIdentifier::Pathing {
        variation: audionimbus::BakedDataVariation::Dynamic,
    };

    let params = audionimbus::PathBakeParams {
        scene: &root,
        probe_batch: &batch,
        identifier: &id,
        num_samples: 32,
        radius: 1.0,
        threshold: 0.1,
        visibility_range: 1000.0,
        path_range: 100.0,
        num_threads: 4,
    };
    let mut callback: Box<dyn FnMut(f32)> = Box::new(|x| println!("progress: {x:.2}"));
    let ctx = &mut callback as *mut _ as *mut std::ffi::c_void;

    audionimbus::bake_path(
        &STEAM_AUDIO_CONTEXT,
        &params,
        Some(audionimbus::CallbackInformation {
            callback: trampoline,
            user_data: ctx,
        }),
    );

    Ok(())
}

extern "C" fn trampoline(progress: f32, ctx: *mut std::ffi::c_void) {
    let closure = unsafe { &mut *(ctx as *mut Box<dyn FnMut(f32)>) };
    closure(progress);
}
