use std::marker::PhantomData;
use std::sync::LazyLock;

use bevy_seedling::sample::SamplePlayer;
use prelude::*;

pub mod nodes;
pub mod scene;
pub mod simulation;
pub mod sources;
mod wrapper;
pub use audionimbus;
pub use audionimbus::Material as SteamAudioMaterial;

pub mod settings;

pub mod prelude {
    pub(crate) use crate::{STEAM_AUDIO_CONTEXT, SteamAudioSystems};
    pub(crate) use bevy_app::prelude::*;
    pub(crate) use bevy_asset::prelude::*;
    pub(crate) use bevy_derive::{Deref, DerefMut};
    pub(crate) use bevy_ecs::{error::Result, prelude::*};
    pub(crate) use bevy_log::prelude::*;
    pub(crate) use bevy_math::prelude::*;
    pub(crate) use bevy_mesh::prelude::*;
    pub(crate) use bevy_platform::prelude::*;
    pub(crate) use bevy_reflect::prelude::*;
    pub(crate) use bevy_tasks::prelude::*;
    pub(crate) use bevy_time::prelude::*;
    pub(crate) use bevy_transform::prelude::*;
    pub(crate) use bevy_utils::prelude::*;

    pub use crate::{
        SteamAudioListener, SteamAudioMaterial, SteamAudioPlugin, nodes::SteamAudioPool,
    };
}

pub struct SteamAudioPlugin {
    _pd: PhantomData<()>,
}

impl Default for SteamAudioPlugin {
    fn default() -> Self {
        Self { _pd: PhantomData }
    }
}

impl Plugin for SteamAudioPlugin {
    fn build(&self, app: &mut App) {
        app.configure_sets(
            PostUpdate,
            (SteamAudioSystems::MeshLifecycle,)
                .chain()
                .after(TransformSystems::Propagate),
        );
        app.add_plugins((
            nodes::plugin,
            simulation::plugin,
            wrapper::plugin,
            scene::plugin,
            settings::plugin,
            sources::plugin,
        ));
    }
}

#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub enum SteamAudioSystems {
    MeshLifecycle,
}

#[derive(Component, Reflect, Debug)]
#[reflect(Component)]
pub struct SteamAudioListener;

pub static STEAM_AUDIO_CONTEXT: LazyLock<audionimbus::Context> = LazyLock::new(|| {
    audionimbus::Context::try_new(&audionimbus::ContextSettings::default()).unwrap()
});

#[derive(Component)]
#[require(Transform, GlobalTransform, SteamAudioPool, SamplePlayer)]
pub struct SteamAudioSamplePlayer {
    flags: audionimbus::SimulationFlags,
}

impl Default for SteamAudioSamplePlayer {
    fn default() -> Self {
        Self {
            flags: audionimbus::SimulationFlags::all(),
        }
    }
}
