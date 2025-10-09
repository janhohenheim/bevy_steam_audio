use std::marker::PhantomData;

use crate::prelude::*;

pub struct Mesh3dBackendPlugin {
    _pd: PhantomData<()>,
}

impl Default for Mesh3dBackendPlugin {
    fn default() -> Self {
        Self { _pd: PhantomData }
    }
}

impl Plugin for Mesh3dBackendPlugin {
    fn build(&self, _app: &mut App) {}
}
