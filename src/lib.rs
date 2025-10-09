use prelude::*;

mod audio;
mod wrapper;

pub mod prelude {
    pub(crate) use bevy_app::prelude::*;
    pub(crate) use bevy_derive::{Deref, DerefMut};
    pub(crate) use bevy_ecs::prelude::*;
    pub(crate) use bevy_log::prelude::*;
    pub(crate) use bevy_math::prelude::*;
    pub(crate) use bevy_transform::prelude::*;
    pub(crate) use bevy_utils::prelude::*;

    pub use crate::{Listener, SteamAudioConfig, SteamAudioPlugin};
}

#[derive(Default)]
pub struct SteamAudioPlugin;

impl Plugin for SteamAudioPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(audio::plugin);
        app.init_resource::<SteamAudioConfig>();
    }
}

#[derive(Component, Debug)]
pub struct Listener;

#[derive(Resource, Default, Debug)]
pub struct SteamAudioConfig;
