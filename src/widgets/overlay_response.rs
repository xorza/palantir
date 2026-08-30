//! What one frame of an overlay body reports, and the value that body
//! produced.

/// Result of [`Popup::show`](crate::Popup::show) and
/// [`Modal::show`](crate::Modal::show).
///
/// One type for both, because a dialog and an anchored panel close for
/// the same two reasons and every host branches on the same predicate.
/// `dismissed` is set when the user asked from outside — an eaten
/// outside-press or an Esc press. `close_requested` is set when a content
/// widget inside the body called [`CloseHandle::close`](crate::CloseHandle::close).
/// `inner` is whatever the body returned, the way
/// [`InnerResponse`](crate::InnerResponse) carries a container's.
#[derive(Copy, Clone, Debug, Default)]
pub struct OverlayResponse<R> {
    pub dismissed: bool,
    pub close_requested: bool,
    pub inner: R,
}

impl<R> OverlayResponse<R> {
    /// `true` when the overlay asked to close this frame, from either
    /// side. The single close-signal predicate overlay-trigger widgets
    /// (`ComboBox`, `ContextMenu`) branch on, so the dismiss contract
    /// lives in one place.
    pub fn closed(&self) -> bool {
        self.dismissed || self.close_requested
    }
}
