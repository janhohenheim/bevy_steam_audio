use bevy::prelude::*;
use bevy_steam_audio::prelude::*;

fn main() {
    App::new()
        .add_plugins((DefaultPlugins, SteamAudioPlugin))
        .run();
}
