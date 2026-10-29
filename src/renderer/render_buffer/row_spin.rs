//! A paint animation's turn as a draw row carries it.

use glam::Vec2;

/// A turn of a whole draw by `angle` radians about `pivot`, in physical px.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct RowSpin {
    pub(crate) pivot: Vec2,
    pub(crate) angle: f32,
}
