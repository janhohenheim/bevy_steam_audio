use crate::prelude::*;
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
        fn vec3_to_vector3(vec3: Vec3) -> audionimbus::Vector3 {
            audionimbus::Vector3::new(vec3.x, vec3.y, vec3.z)
        }
        audionimbus::CoordinateSystem {
            right: vec3_to_vector3(self.right),
            up: vec3_to_vector3(self.up),
            ahead: vec3_to_vector3(self.ahead),
            origin: vec3_to_vector3(self.origin),
        }
    }
}
