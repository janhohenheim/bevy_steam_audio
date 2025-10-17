use crate::{prelude::*, wrapper::ToSteamAudioVec3 as _};
use firewheel::diff::{Diff, Patch, RealtimeClone};

pub(super) fn plugin(app: &mut App) {
    let _ = app;
}

#[derive(Debug, Patch, Diff, Clone, Copy, Default, PartialEq, RealtimeClone, Reflect)]
pub struct AudionimbusCoordinateSystem {
    pub right: Vec3,
    pub up: Vec3,
    pub ahead: Vec3,
    pub origin: Vec3,
}

impl AudionimbusCoordinateSystem {
    pub fn from_bevy_transform(transform: Transform) -> Self {
        let listener_position = transform.translation;

        let listener_orientation_right = transform.right();
        let listener_orientation_up = transform.up();
        let listener_orientation_ahead = transform.forward();
        Self {
            right: listener_orientation_right.into(),
            up: listener_orientation_up.into(),
            ahead: listener_orientation_ahead.into(),
            origin: listener_position,
        }
    }

    pub fn to_audionimbus(self) -> audionimbus::CoordinateSystem {
        audionimbus::CoordinateSystem {
            right: self.right.to_steam_audio_vec3(),
            up: self.up.to_steam_audio_vec3(),
            ahead: self.ahead.to_steam_audio_vec3(),
            origin: self.origin.to_steam_audio_vec3(),
        }
    }
}
