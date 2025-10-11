use crate::{SteamAudioSamplePlayer, prelude::*, simulation::AudionimbusSimulator};

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<ToSetup>();
    app.add_observer(remove_source);
    app.add_systems(
        PostUpdate,
        (
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
    simulator: Res<AudionimbusSimulator>,
    settings: Query<&SteamAudioSamplePlayer>,
    mut errors: Local<Vec<String>>,
    names: Query<NameOrEntity>,
) -> Result {
    errors.clear();
    for entity in to_setup.drain(..) {
        let name = names.get(entity).unwrap();
        let settings = match settings.get(entity) {
            Ok(settings) => settings,
            Err(_) => {
                errors.push(format!("{name} Failed to get SteamAudioSamplePlayer"));
                continue;
            }
        };
        let settings = audionimbus::SourceSettings {
            flags: settings.flags,
        };
        let source = match audionimbus::Source::try_new(&simulator.read().unwrap(), &settings) {
            Ok(source) => source,
            Err(err) => {
                errors.push(format!("{name} Failed to create AudionimbusSource: {err}"));
                continue;
            }
        };
        simulator.write().unwrap().add_source(&source);
        commands.entity(entity).insert(AudionimbusSource(source));
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("\n").into())
    }
}

fn remove_source(remove: On<Remove, SteamAudioSamplePlayer>, mut commands: Commands) {
    commands
        .entity(remove.entity)
        .try_remove::<AudionimbusSource>();
}
