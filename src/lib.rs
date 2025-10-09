use std::marker::PhantomData;

use prelude::*;

mod backend;
mod decoder;
mod encoder;
pub mod mesh_backend;
mod simulation;
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
        app.add_plugins((encoder::plugin, decoder::plugin, simulation::plugin));
        app.init_resource::<SteamAudioConfig>();
    }
}

#[derive(Component, Reflect, Debug)]
#[reflect(Component)]
pub struct Listener;

#[derive(Resource, Reflect, Default, Debug)]
#[reflect(Resource)]
pub struct SteamAudioConfig;

pub(crate) const FRAME_SIZE: u32 = 256;
pub(crate) const AMBISONICS_ORDER: u32 = 2;
pub(crate) const AMBISONICS_NUM_CHANNELS: u32 = (AMBISONICS_ORDER + 1).pow(2);
pub(crate) const GAIN_FACTOR_DIRECT: f32 = 1.0;
pub(crate) const GAIN_FACTOR_REFLECTIONS: f32 = 0.3;
pub(crate) const GAIN_FACTOR_REVERB: f32 = 0.1;
