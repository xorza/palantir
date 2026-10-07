//! The pointer shape a widget asks the host to show.

use crate::primitives::layout::axis::Axis;

/// The mouse cursor a widget wants shown this frame, requested via [`Ui::set_cursor`](crate::Ui::set_cursor). A backend-agnostic subset (the winit mapping lives in the host) that grows as widgets need.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CursorIcon {
    /// The platform arrow; every frame resets to it.
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
    /// Move the thing under the pointer, on both axes.
    Move,
    /// Precise aim — a picker, a canvas tool.
    Crosshair,
    /// Horizontal resize (a vertical divider).
    EwResize,
    /// Vertical resize (a horizontal divider).
    NsResize,
    /// The gesture under way cannot land here.
    NotAllowed,
}

impl CursorIcon {
    /// The resize cursor for a divider dragged **along** `axis`. Quarter-turn: dragging along X moves a *vertical* divider, wanting the east-west arrows; named once because it is easy to get backwards.
    pub(crate) const fn resize_along(axis: Axis) -> Self {
        match axis {
            Axis::X => Self::EwResize,
            Axis::Y => Self::NsResize,
        }
    }
}
