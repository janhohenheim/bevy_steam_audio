use crate::prelude::*;

pub(super) fn plugin(app: &mut App) {
    let _ = app;
}

pub trait ToSteamAudioTransform: Copy {
    fn to_steam_audio_transform(self) -> audionimbus::Matrix<f32, 4, 4>;
}

impl ToSteamAudioTransform for GlobalTransform {
    fn to_steam_audio_transform(self) -> audionimbus::Matrix<f32, 4, 4> {
        let row_major = self.to_matrix().transpose().to_cols_array_2d();
        audionimbus::Matrix::new(row_major)
    }
}

pub trait ToSteamAudioVec3: Copy {
    fn to_steam_audio_vec3(self) -> audionimbus::Vector3;
}

impl ToSteamAudioVec3 for Vec3 {
    fn to_steam_audio_vec3(self) -> audionimbus::Vector3 {
        audionimbus::Vector3::new(self.x, self.y, self.z)
    }
}
