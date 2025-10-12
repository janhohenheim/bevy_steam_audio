use crate::prelude::*;

pub(super) fn plugin(app: &mut App) {
    let _ = app;
}

#[derive(Deref, DerefMut)]
pub(crate) struct ChannelPtrs(Box<[*mut f32]>);

// Safety: We only access these inside a single processor, which runs in a single thread
unsafe impl Send for ChannelPtrs {}

impl ChannelPtrs {
    pub fn new(size: usize) -> Self {
        Self(
            core::iter::repeat_with(|| core::ptr::null_mut())
                .take(size)
                .collect(),
        )
    }
}
