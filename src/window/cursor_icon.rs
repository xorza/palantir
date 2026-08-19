//! The pointer shape a widget asks the host to show.

use crate::layout::axis::Axis;

/// The mouse cursor a widget wants shown this frame, requested through
/// [`Ui::set_cursor`](crate::Ui::set_cursor). Backend-agnostic subset of
/// the platform cursors (the winit mapping lives in the host); grows
/// variants as widgets need them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CursorIcon {
    /// The platform arrow — what every frame resets to.
    #[default]
    Default,
    /// Clickable affordance (hand).
    Pointer,
    /// Text caret (I-beam).
    Text,
    /// Open hand: a grabbable surface.
    Grab,
    /// Closed hand: a grab in progress.
    Grabbing,
    Move,
    Crosshair,
    /// Horizontal resize (a vertical divider).
    EwResize,
    /// Vertical resize (a horizontal divider).
    NsResize,
    NotAllowed,
}

impl CursorIcon {
    /// The double-headed resize cursor for a divider the pointer drags
    /// **along** `axis`. Note the quarter-turn: dragging along X moves a
    /// *vertical* divider, which wants the east-west arrows. Getting that
    /// backwards is easy enough by hand that the mapping is worth naming
    /// once.
    pub(crate) fn resize_along(axis: Axis) -> Self {
        match axis {
            Axis::X => Self::EwResize,
            Axis::Y => Self::NsResize,
        }
    }
}
