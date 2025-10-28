use bevy_ecs::entity_disabling::Disabled;
use bevy_seedling::{
    node::follower::FollowerOf,
    prelude::{AudioEvents, EffectOf, EffectsQuery, SampleEffects},
};
use firewheel::{diff::EventQueue as _, event::NodeEventType};

use crate::{prelude::*, simulation::AudionimbusSimulator};

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<ToSetup>()
        .init_resource::<SourcesToRemove>();
    app.add_observer(remove_steam_audio_source)
        .add_observer(queue_audionimbus_source_init)
        .add_observer(send_source_to_processor);
    app.add_systems(
        PostUpdate,
        (
            send_source_to_reverb_processor,
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

fn send_source_to_processor(
    add: On<Add, AudionimbusSource>,
    effects: Query<(&AudionimbusSource, &SampleEffects), Allow<Disabled>>,
    mut events: Query<&mut AudioEvents, (With<SteamAudioNode>, Allow<Disabled>)>,
) -> Result {
    let (source, effects) = effects.get(add.entity)?;
    let mut events = events.get_effect_mut(effects)?;
    let source: audionimbus::Source = source.0.clone();
    events.push(NodeEventType::custom(Some(source)));
    Ok(())
}

fn send_source_to_reverb_processor(
    source: Res<ListenerSource>,
    mut events: Single<&mut AudioEvents, With<SteamAudioReverbNode>>,
) {
    if source.is_changed() {
        let source: audionimbus::Source = source.0.clone();
        events.push(NodeEventType::custom(Some(source)));
    }
}

#[derive(Resource, Default, Deref, DerefMut)]
struct ToSetup(Vec<Entity>);

fn queue_audionimbus_source_init(
    add: On<Add, FollowerOf>,
    follower_of: Query<&FollowerOf, Allow<Disabled>>,
    effect_of: Query<&EffectOf, (With<SteamAudioNode>, Allow<Disabled>)>,
    mut to_setup: ResMut<ToSetup>,
) {
    if let Ok(follower_of) = follower_of.get(add.entity)
        && let Ok(effect_of) = effect_of.get(follower_of.0)
    {
        to_setup.push(effect_of.0);
    }
}

fn init_audionimbus_sources(
    mut commands: Commands,
    mut to_setup: ResMut<ToSetup>,
    mut simulator: ResMut<AudionimbusSimulator>,
    mut errors: Local<Vec<String>>,
    names: Query<NameOrEntity>,
    mut to_retry: Local<Vec<Entity>>,
) -> Result {
    errors.clear();
    if to_setup.is_empty() {
        return Ok(());
    }
    let Ok(simulator) = simulator.get().try_read() else {
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
    mut simulator: ResMut<AudionimbusSimulator>,
) {
    if to_remove.is_empty() {
        return;
    }
    let Ok(simulator) = simulator.get().try_read() else {
        return;
    };
    for source in to_remove.0.drain(..) {
        simulator.remove_source(&source);
    }
}
