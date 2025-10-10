use crate::prelude::*;

pub(super) fn plugin(app: &mut App) {
    let _ = app;
}

#[derive(Deref, DerefMut)]
pub(crate) struct ChannelPtrs(Vec<*mut f32>);
unsafe impl Send for ChannelPtrs {}

impl From<Vec<*mut f32>> for ChannelPtrs {
    fn from(value: Vec<*mut f32>) -> Self {
        Self(value)
    }
}
