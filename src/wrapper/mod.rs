pub(crate) mod channel_ptrs;
pub(crate) mod coordinate_system;

pub(crate) use channel_ptrs::*;
pub(crate) use coordinate_system::*;

use crate::prelude::*;

pub(super) fn plugin(app: &mut App) {
    app.add_plugins((coordinate_system::plugin, channel_ptrs::plugin));
}
