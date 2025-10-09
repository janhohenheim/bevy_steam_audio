use crate::prelude::*;

mod backend;
pub mod mesh_backend;
mod trimesh;

pub use backend::*;
pub use trimesh::*;

pub(super) fn plugin(app: &mut App) {
    app.add_plugins((backend::plugin, mesh_backend::plugin, trimesh::plugin));
}
