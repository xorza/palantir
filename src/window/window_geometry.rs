//! A window's live size and placement, as the app persists it.

use glam::{IVec2, UVec2};

/// A window's live geometry, assembled on demand by
/// [`Ui::window_geometry`](crate::Ui::window_geometry) so the app can
/// persist and restore size / position across launches. A computed view,
/// not stored state: the size comes from the frame's `Display` (the single
/// source of truth for surface size), the position + maximized flag from
/// the host's window-manager facts. Backend-agnostic (no winit types),
/// matching [`WindowConfig`](crate::window::window_config::WindowConfig)'s vocabulary: logical size, physical position.
#[derive(Clone, Copy, Debug, Default)]
pub struct WindowGeometry {
    /// Inner (content) size in logical pixels — DPI-independent, so it
    /// round-trips through [`WindowConfig::inner_size()`](crate::window::window_config::WindowConfig::inner_size) unchanged across
    /// monitors of different scale.
    pub inner_size: UVec2,
    /// Outer position in physical pixels, or `None` when the platform
    /// doesn't report it (Wayland clients can't know their absolute
    /// position). Feeds [`WindowConfig::position()`](crate::window::window_config::WindowConfig::position) on restore.
    pub outer_position: Option<IVec2>,
    /// Whether the window is currently maximized.
    pub maximized: bool,
}
