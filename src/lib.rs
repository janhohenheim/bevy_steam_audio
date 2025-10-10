use std::marker::PhantomData;

use prelude::*;

mod nodes;
pub mod scene;
mod simulation;
mod wrapper;
pub use audionimbus;
pub use audionimbus::Material as SteamAudioMaterial;

pub mod prelude {
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

    pub use crate::{Listener, SteamAudioConfig, SteamAudioMaterial, SteamAudioPlugin};
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
        app.add_plugins((
            nodes::plugin,
            simulation::plugin,
            wrapper::plugin,
            scene::plugin,
        ));
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
pub(crate) const GAIN_FACTOR_DIRECT: f32 = 1.0;
pub(crate) const GAIN_FACTOR_REFLECTIONS: f32 = 0.3;
pub(crate) const GAIN_FACTOR_REVERB: f32 = 0.1;
