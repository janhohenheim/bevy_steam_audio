use std::marker::PhantomData;
use std::sync::LazyLock;

use bevy_seedling::sample::SamplePlayer;
use prelude::*;

pub mod nodes;
pub mod probes;
pub mod scene;
pub mod simulation;
pub mod sources;
pub mod wrapper;
pub use audionimbus;
#[cfg(feature = "debug")]
pub mod debug;
pub mod settings;

pub mod prelude {
    #[cfg(feature = "debug")]
    pub(crate) use crate::debug::SpawnSteamAudioGizmo;
    pub(crate) use crate::{STEAM_AUDIO_CONTEXT, SteamAudioSystems};
    pub(crate) use bevy_app::prelude::*;
    pub(crate) use bevy_asset::prelude::*;
    pub(crate) use bevy_derive::{Deref, DerefMut};
    pub(crate) use bevy_ecs::{error::Result, prelude::*};
    #[cfg(feature = "debug")]
    pub(crate) use bevy_gizmos::prelude::*;
    pub(crate) use bevy_log::prelude::*;
    pub(crate) use bevy_math::prelude::*;
    pub(crate) use bevy_mesh::prelude::*;
    pub(crate) use bevy_platform::prelude::*;
    pub(crate) use bevy_reflect::prelude::*;
    pub(crate) use bevy_tasks::prelude::*;
    pub(crate) use bevy_time::prelude::*;
    pub(crate) use bevy_transform::prelude::*;
    pub(crate) use bevy_utils::prelude::*;

    #[cfg(feature = "debug")]
    pub use crate::debug::SteamAudioDebugPlugin;
    pub use crate::{
        SteamAudioListener, SteamAudioPlugin, SteamAudioSamplePlayer,
        nodes::{
            AmbisonicDecodeNode, SteamAudioNode, SteamAudioPool, SteamAudioReverbNode,
            SteamAudioReverbPool,
        },
        probes::GenerateProbes,
        scene::Static,
        settings::{
            SteamAudioDirectQuality, SteamAudioPathingQuality, SteamAudioQuality,
            SteamAudioReflectionsQuality,
        },
        wrapper::SteamAudioMaterial,
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
            (
                SteamAudioSystems::CreateSimulator,
                SteamAudioSystems::MeshLifecycle,
                SteamAudioSystems::UpdateTransforms,
                SteamAudioSystems::UpdateSources,
                SteamAudioSystems::RunSimulator,
                SteamAudioSystems::GenerateProbes,
                SteamAudioSystems::Gizmos,
            )
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
            probes::plugin,
        ));
    }
}

#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub enum SteamAudioSystems {
    CreateSimulator,
    MeshLifecycle,
    UpdateTransforms,
    GenerateProbes,
    UpdateSources,
    RunSimulator,
    Gizmos,
}

#[derive(Component, Reflect, Debug)]
#[reflect(Component)]
#[require(Transform, GlobalTransform)]
pub struct SteamAudioListener;

pub static STEAM_AUDIO_CONTEXT: LazyLock<audionimbus::Context> = LazyLock::new(|| {
    audionimbus::Context::try_new(&audionimbus::ContextSettings::default()).unwrap()
});

#[derive(Component)]
#[require(Transform, GlobalTransform, SamplePlayer)]
pub struct SteamAudioSamplePlayer {
    pub flags: audionimbus::SimulationFlags,
}

impl Default for SteamAudioSamplePlayer {
    fn default() -> Self {
        Self {
            flags: audionimbus::SimulationFlags::all(),
        }
    }
}
