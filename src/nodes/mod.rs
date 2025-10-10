use crate::prelude::*;
pub(crate) mod decoder;
pub(crate) mod encoder;
pub(crate) mod reverb;

pub use decoder::*;
pub use encoder::*;

pub(super) fn plugin(app: &mut App) {
    app.add_plugins((decoder::plugin, encoder::plugin, reverb::plugin));
}
