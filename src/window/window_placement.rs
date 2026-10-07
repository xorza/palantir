//! Where a window sits on the desktop.

use glam::IVec2;

/// A window's outer position and maximized state, the pair that survives a restart.
///
/// Shared by what the windowing system reports, what the host copies into the recorder, [`WindowGeometry`](crate::WindowGeometry) and [`WindowConfig`](crate::WindowConfig). Size is not here: a live window always has one and a config may not, under different types.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WindowPlacement {
    /// Outer position of the window frame in **physical** pixels, unambiguous across mixed-DPI monitors. `None` where the platform reports none (Wayland); on restore the platform places the window.
    pub position: Option<IVec2>,
    /// Whether the window is maximized. On restore the host applies it and keeps the configured inner size for un-maximizing.
    pub maximized: bool,
}
