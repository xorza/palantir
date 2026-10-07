//! What one frame of an overlay body reports, and the value that body produced.

use crate::primitives::identity::widget_id::WidgetId;

/// Result of [`Popup::show`](crate::Popup::show) and
/// [`Modal::show`](crate::Modal::show).
///
/// One type for both, because a dialog and an anchored panel close for the same two reasons and every
/// host branches on the same predicate.
/// `dismissed` is set when the user asked from outside — an eaten
/// outside-press or an Esc press. `close_requested` is set when a content
/// widget inside the body called [`CloseHandle::close`](crate::CloseHandle::close).
/// `inner` is whatever the body returned, the way
/// [`InnerResponse`](crate::InnerResponse) carries a container's.
///
/// **No [`Response`](crate::Response) beside it**, as with [`TooltipResponse`](crate::TooltipResponse):
/// an overlay's own node is placement and a scrim, not something anyone interacts with (the widgets
/// *inside* the body each return their own). A `Ui` borrow on this type would also cost the `Copy` and
/// `Default` that let a trigger widget hold a closed overlay's result without a branch.
#[derive(Copy, Clone, Debug, Default)]
pub struct OverlayResponse<R> {
    /// The overlay's own id — the node it records its body under, and what
    /// [`Ui::focus_first_within`](crate::Ui::focus_first_within) takes to
    /// move focus into it. The default id on a frame the overlay was not recorded.
    pub id: WidgetId,
    /// The user asked from outside — an eaten outside-press, or Escape.
    pub dismissed: bool,
    /// A widget inside the body called
    /// [`CloseHandle::close`](crate::CloseHandle::close).
    pub close_requested: bool,
    /// What the body returned.
    pub inner: R,
}

impl<R> OverlayResponse<R> {
    /// `true` when the overlay asked to close this frame, from either side: the one close-signal
    /// predicate overlay-trigger widgets (`ComboBox`, `ContextMenu`) branch on.
    pub const fn closed(&self) -> bool {
        self.dismissed || self.close_requested
    }
}
