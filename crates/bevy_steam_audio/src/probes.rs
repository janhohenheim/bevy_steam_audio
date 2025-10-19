use bevy_camera::primitives::Aabb;
use bevy_math::bounding::Aabb3d;

use crate::{
    prelude::*, scene::SteamAudioRootScene, settings::SteamAudioPathBakingSettings,
    simulation::AudionimbusSimulator, wrapper::ToSteamAudioTransform,
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
    root: ResMut<SteamAudioRootScene>,
    mut commands: Commands,
    mut simulator: ResMut<AudionimbusSimulator>,
    probe_batch: Option<Res<SteamAudioProbeBatch>>,
    pathing_settings: Res<SteamAudioPathBakingSettings>,
    quality: Res<SteamAudioQuality>,
) -> Result {
    let mut global_aabb = None;
    let Some(generate) = generate_probes.drain().last() else {
        return Ok(());
    };
    let Ok(mut simulator) = simulator.get().try_write() else {
        // Simulator is in use, try again next frame
        generate_probes.write(generate);
        return Ok(());
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
                    Aabb3d { min, max }
                })
        })
    };
    // Transform is applied to an *axis-aligned bounding box*
    let scale = aabb.max - aabb.min;
    let translation = aabb.min + scale / 2.0;
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
    debug!("Generated {} probes", array.num_probes());

    let mut batch = audionimbus::ProbeBatch::try_new(&STEAM_AUDIO_CONTEXT)?;
    batch.add_probe_array(&array);
    batch.commit();

    if let Some(old_batch) = probe_batch.as_ref() {
        simulator.remove_probe_batch(old_batch);
    }
    simulator.add_probe_batch(&batch);
    simulator.commit();

    let bake_params = audionimbus::PathBakeParams {
        scene: &root,
        probe_batch: &batch,
        identifier: &audionimbus::BakedDataIdentifier::Pathing {
            variation: audionimbus::BakedDataVariation::Dynamic,
        },
        num_samples: quality.pathing.num_visibility_samples,
        radius: pathing_settings.visibility_radius,
        threshold: pathing_settings.visibility_threshold,
        path_range: pathing_settings.path_range,
        visibility_range: pathing_settings.visibility_range,
        num_threads: 4,
    };
    audionimbus::bake_path(
        &STEAM_AUDIO_CONTEXT,
        &bake_params,
        Some(audionimbus::CallbackInformation {
            callback: progress_callback,
            user_data: std::ptr::null_mut(),
        }),
    );

    commands.insert_resource(SteamAudioProbeBatch(batch));

    Ok(())
}

unsafe extern "C" fn progress_callback(progress: f32, _user_data: *mut std::ffi::c_void) {
    debug!("Pathing progress: {:.2}%", progress * 100.0);
}
