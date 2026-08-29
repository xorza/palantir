//! Where a window sits on the desktop.

use glam::IVec2;

/// A window's outer position and maximized state — the pair that survives
/// a restart.
///
/// One type because four vocabularies used to spell it out one field at a
/// time: what the windowing system reports, what the host copies into the
/// recorder each frame, what an app persists through
/// [`WindowGeometry`](crate::WindowGeometry), and what it restores through
/// [`WindowConfig`](crate::WindowConfig). Two of those called the position
/// `position` and one called it `outer_position`, and the documented
/// persist/restore round trip was a field-by-field copy across the
/// difference. It is one copy now, and a field added here reaches every
/// hop.
///
/// Size is deliberately not here. A live window always has one and a
/// config may leave it to the platform, so the two carry it under
/// different types — `UVec2` against `Option<UVec2>` — and folding them
/// together would mean one of the two lying.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WindowPlacement {
    /// Outer position of the window frame, in **physical** pixels.
    ///
    /// Physical rather than logical because a saved position is only
    /// unambiguous across mixed-DPI monitors in device pixels. `None`
    /// where the platform does not report one — a Wayland client cannot
    /// know its absolute position — and `None` on restore lets the
    /// platform place the window.
    pub position: Option<IVec2>,
    /// Whether the window is maximized. On restore the host applies it and
    /// holds the configured inner size as the size to return to when the
    /// user un-maximizes.
    pub maximized: bool,
}
