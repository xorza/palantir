//! A window's live size and placement, as the app persists it.

use crate::primitives::geometry::size::Size;
use crate::window::window_placement::WindowPlacement;

/// A window's live geometry, assembled by [`Ui::window_geometry`](crate::Ui::window_geometry) for persisting and restoring; shares [`WindowPlacement`] with [`WindowConfig`](crate::window::window_config::WindowConfig), so restore is a copy.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WindowGeometry {
    /// Inner (content) size in logical pixels.
    pub inner_size: Size,
    /// Where the window sits.
    pub placement: WindowPlacement,
}
