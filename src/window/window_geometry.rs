//! A window's live size and placement, as the app persists it.

use crate::window::window_placement::WindowPlacement;
use glam::UVec2;

/// A window's live geometry, assembled on demand by
/// [`Ui::window_geometry`](crate::Ui::window_geometry) so the app can
/// persist and restore size and placement across launches. A computed
/// view, not stored state: the size comes from the frame's `Display` (the
/// single source of truth for surface size) and the placement from the
/// host's window-manager facts. Backend-agnostic (no winit types), and it
/// shares [`WindowPlacement`] with
/// [`WindowConfig`](crate::window::window_config::WindowConfig) rather
/// than restating it, so the restore is a copy.
#[derive(Clone, Copy, Debug, Default)]
pub struct WindowGeometry {
    /// Inner (content) size in logical pixels — DPI-independent, so it
    /// round-trips through [`WindowConfig::inner_size()`](crate::window::window_config::WindowConfig::inner_size) unchanged across
    /// monitors of different scale.
    pub inner_size: UVec2,
    /// Where the window sits, as
    /// [`WindowConfig::placement`](crate::window::window_config::WindowConfig::placement)
    /// takes it back on restore.
    pub placement: WindowPlacement,
}
