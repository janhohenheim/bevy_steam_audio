use bevy_ecs::entity_disabling::Disabled;
use bevy_seedling::{pool::SamplerPool, prelude::EffectOf, sample::SamplePlayer};

use crate::{prelude::*, simulation::AudionimbusSimulator};

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<ToSetup>().init_resource::<ToRemove>();
    app.add_observer(remove_steam_audio_source)
        .add_observer(queue_audionimbus_source_init);
    app.add_systems(
        PostUpdate,
        (
            drain_to_remove,
            init_audionimbus_sources.run_if(resource_exists::<AudionimbusSimulator>),
        )
            .chain()
            .in_set(SteamAudioSystems::UpdateSources),
    );
}

#[derive(Resource, Deref, DerefMut)]
pub struct ListenerSource(pub(crate) audionimbus::Source);

#[derive(Component, Deref, DerefMut)]
#[require(Transform, GlobalTransform)]
pub struct AudionimbusSource(pub(crate) audionimbus::Source);

#[derive(Resource, Default, Deref, DerefMut)]
struct ToSetup(Vec<Entity>);

fn queue_audionimbus_source_init(
    add: On<Add, SteamAudioNode>,
    effect_of: Query<&EffectOf, Allow<Disabled>>,
    mut to_setup: ResMut<ToSetup>,
) {
    if let Ok(effect_of) = effect_of.get(add.entity) {
        to_setup.push(effect_of.0);
    }
}

fn init_audionimbus_sources(
    mut commands: Commands,
    mut to_setup: ResMut<ToSetup>,
    simulator: ResMut<AudionimbusSimulator>,
    mut errors: Local<Vec<String>>,
    names: Query<NameOrEntity>,
    mut to_retry: Local<Vec<Entity>>,
) -> Result {
    errors.clear();
    if to_setup.is_empty() {
        return Ok(());
    }
    let Ok(simulator) = simulator.try_read() else {
        return Ok(());
    };
    for entity in to_setup.drain(..) {
        if commands.get_entity(entity).is_err() {
            continue;
        }
        let name = names.get(entity).unwrap();

        let source = match audionimbus::Source::try_new(
            &simulator,
            &audionimbus::SourceSettings {
                flags: audionimbus::SimulationFlags::all(),
            },
        ) {
            Ok(source) => source,
            Err(err) => {
                errors.push(format!("{name} Failed to create AudionimbusSource: {err}"));
                continue;
            }
        };
        simulator.add_source(&source);
        commands
            .entity(entity)
            .try_insert(AudionimbusSource(source));
    }
    for entity in to_retry.drain(..) {
        to_setup.push(entity);
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("\n").into())
    }
}

fn remove_steam_audio_source(
    remove: On<Remove, AudionimbusSource>,
    source: Query<&AudionimbusSource, Allow<Disabled>>,
    mut to_remove: ResMut<ToRemove>,
) -> Result {
    let source = source.get(remove.entity)?;
    to_remove.0.push(source.0.clone());
    Ok(())
}

#[derive(Resource, Default, Deref, DerefMut)]
struct ToRemove(Vec<audionimbus::Source>);

fn drain_to_remove(mut to_remove: ResMut<ToRemove>, simulator: ResMut<AudionimbusSimulator>) {
    if to_remove.is_empty() {
        return;
    }
    // Todo: make this `read` once <https://github.com/MaxenceMaire/audionimbus/pull/30> is released
    let Ok(mut simulator) = simulator.try_write() else {
        return;
    };
    for source in to_remove.0.drain(..) {
        simulator.remove_source(&source);
    }
}
