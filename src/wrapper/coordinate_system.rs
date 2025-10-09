use crate::prelude::*;
use firewheel::diff::{Diff, Patch, RealtimeClone};

pub(super) fn plugin(app: &mut App) {
    let _ = app;
}

#[derive(Debug, Patch, Diff, Clone, Copy, Default, RealtimeClone)]
pub(crate) struct AudionimbusCoordinateSystem {
    right: Vec3,
    up: Vec3,
    ahead: Vec3,
    origin: Vec3,
}

impl AudionimbusCoordinateSystem {
    pub(crate) fn from_bevy_transform(transform: Transform) -> Self {
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

    pub(crate) fn to_audionimbus(self) -> audionimbus::CoordinateSystem {
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
