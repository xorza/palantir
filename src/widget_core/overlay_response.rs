//! What one frame of an overlay body reports, and the value that body produced.

use crate::primitives::identity::widget_id::WidgetId;

/// Result of [`Popup::show`](crate::Popup::show) and [`Modal::show`](crate::Modal::show), and
/// inside an `Option`, `None` while closed, of the overlays a trigger opens:
/// [`Menu::show`](crate::Menu::show) and [`PopupTrigger::show`](crate::PopupTrigger::show), as
/// egui's `Popup::show` returns `Option<InnerResponse<R>>`.
///
/// One type for every overlay, because a dialog and an anchored panel close for the same two
/// reasons and every host branches on the same predicate, [`Self::closed`]. `inner` is whatever the
/// body returned, the way [`InnerResponse`](crate::InnerResponse) carries a container's.
///
/// **No [`Response`](crate::Response) beside it**, as with [`TooltipResponse`](crate::TooltipResponse):
/// an overlay's own node is placement and a scrim, not something anyone interacts with (the widgets
/// *inside* the body each return their own).
#[derive(Copy, Clone, Debug)]
pub struct OverlayResponse<R> {
    /// The overlay's own id — the node it records its body under, and what
    /// [`Ui::focus_first_within`](crate::Ui::focus_first_within) takes to
    /// move focus into it.
    pub id: WidgetId,
    /// The overlay shows this frame and did not the frame before: the frame to give its default
    /// control the focus, as a dialog's default button takes it.
    pub opened: bool,
    /// How the overlay closed this frame, if it did.
    pub close: Option<OverlayClose>,
    /// What the body returned.
    pub inner: R,
}

/// How an overlay closed: from outside or from its body.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OverlayClose {
    /// The user asked from outside: an eaten outside press, or Escape.
    Dismissed,
    /// The body asked: a [`CloseHandle::close`](crate::CloseHandle::close), or an activated
    /// [`MenuItem`](crate::MenuItem). Wins over a dismissal in the same frame, as the body acted.
    Requested,
}

impl<R> OverlayResponse<R> {
    /// `true` when the overlay closed this frame, from either side: the one close-signal
    /// predicate overlay-trigger widgets branch on.
    pub const fn closed(&self) -> bool {
        self.close.is_some()
    }
}

impl OverlayClose {
    /// The close of a frame whose body `requested` one and whose user `dismissed` the overlay;
    /// `None` for neither.
    pub(crate) const fn of(requested: bool, dismissed: bool) -> Option<Self> {
        if requested {
            Some(Self::Requested)
        } else if dismissed {
            Some(Self::Dismissed)
        } else {
            None
        }
    }
}
