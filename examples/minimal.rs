use bevy::prelude::*;
use bevy_seedling::prelude::*;
use bevy_steam_audio::{mesh_backend::Mesh3dBackendPlugin, prelude::*};

fn main() {
    App::new()
        .add_plugins((
            DefaultPlugins,
            SeedlingPlugin::default(),
            SteamAudioPlugin::default(),
            Mesh3dBackendPlugin::default(),
        ))
        .run();
}
