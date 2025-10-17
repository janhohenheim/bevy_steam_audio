use bevy_ecs::entity_disabling::Disabled;
use bevy_seedling::sample::SamplePlayer;

use crate::{SteamAudioSamplePlayer, prelude::*, simulation::AudionimbusSimulator};

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<ToSetup>()
        .init_resource::<SourcesToRemove>();
    app.add_observer(remove_source)
        .add_observer(remove_sample_player)
        .add_observer(remove_steam_audio_source);
    app.add_systems(
        PostUpdate,
        (
            drain_to_remove,
            queue_audionimbus_source_mutation,
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

fn queue_audionimbus_source_mutation(
    players: Query<(Entity, Ref<SteamAudioSamplePlayer>)>,
    mut to_setup: ResMut<ToSetup>,
) {
    for (entity, player) in players.iter() {
        if player.is_changed() {
            to_setup.push(entity);
        }
    }
}

fn init_audionimbus_sources(
    mut commands: Commands,
    mut to_setup: ResMut<ToSetup>,
    simulator: ResMut<AudionimbusSimulator>,
    settings: Query<&SteamAudioSamplePlayer>,
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
        let settings = match settings.get(entity) {
            Ok(settings) => settings,
            Err(err) => {
                errors.push(format!(
                    "{name} Failed to get SteamAudioSamplePlayer: {err}"
                ));
                continue;
            }
        };
        let settings = audionimbus::SourceSettings {
            flags: settings.flags,
        };
        let source = match audionimbus::Source::try_new(&simulator, &settings) {
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

fn remove_sample_player(remove: On<Remove, SamplePlayer>, mut commands: Commands) {
    commands
        .entity(remove.entity)
        .try_remove::<SteamAudioSamplePlayer>();
}

fn remove_source(remove: On<Remove, SteamAudioSamplePlayer>, mut commands: Commands) {
    commands
        .entity(remove.entity)
        .try_remove::<AudionimbusSource>();
}

fn remove_steam_audio_source(
    remove: On<Remove, AudionimbusSource>,
    source: Query<&AudionimbusSource, Allow<Disabled>>,
    mut to_remove: ResMut<SourcesToRemove>,
) -> Result {
    let source = source.get(remove.entity)?;
    to_remove.0.push(source.0.clone());
    Ok(())
}

#[derive(Resource, Default, Deref, DerefMut)]
pub(crate) struct SourcesToRemove(pub(crate) Vec<audionimbus::Source>);

fn drain_to_remove(
    mut to_remove: ResMut<SourcesToRemove>,
    simulator: ResMut<AudionimbusSimulator>,
) {
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
