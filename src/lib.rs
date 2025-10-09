use std::marker::PhantomData;

use prelude::*;

mod audio;
mod backend;
pub mod mesh_backend;
mod wrapper;

pub use wrapper::*;

pub mod prelude {
    pub(crate) use bevy_app::prelude::*;
    pub(crate) use bevy_derive::{Deref, DerefMut};
    pub(crate) use bevy_ecs::prelude::*;
    pub(crate) use bevy_log::prelude::*;
    pub(crate) use bevy_math::prelude::*;
    pub(crate) use bevy_platform::prelude::*;
    pub(crate) use bevy_reflect::prelude::*;
    pub(crate) use bevy_transform::prelude::*;
    pub(crate) use bevy_utils::prelude::*;

    pub use crate::{Listener, SteamAudioConfig, SteamAudioPlugin, material::SteamAudioMaterial};
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
        app.add_plugins(audio::plugin);
        app.init_resource::<SteamAudioConfig>();
    }
}

#[derive(Component, Reflect, Debug)]
#[reflect(Component)]
pub struct Listener;

#[derive(Resource, Reflect, Default, Debug)]
#[reflect(Resource)]
pub struct SteamAudioConfig;
