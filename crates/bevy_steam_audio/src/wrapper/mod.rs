pub(crate) mod channel_ptrs;
pub(crate) mod coordinate_system;
pub(crate) mod material;
pub(crate) mod mesh;
pub(crate) mod transform;

pub(crate) use channel_ptrs::*;
pub use coordinate_system::*;
pub use material::*;
pub use mesh::*;
pub use transform::*;

use crate::prelude::*;

pub(super) fn plugin(app: &mut App) {
    app.add_plugins((
        coordinate_system::plugin,
        channel_ptrs::plugin,
        mesh::plugin,
        transform::plugin,
        material::plugin,
    ));
}
