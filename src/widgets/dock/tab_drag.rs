//! The tab a pointer is carrying, and where the last frame showed it would land.

use crate::widgets::dock::pane_geometry::DropTarget;

/// A tab mid-drag, kept on the dock's own [`Ui`](crate::Ui) state row since only the widget can act on a half-finished drag.
///
/// `tab` is an identity, so an undo rearranging the strip can't strand the gesture, and the ghost label is re-read from the tab's label at paint. `target` is the drop the last frame classified under the view's split policy and showed: the scan that sees the release lands the tab there, as the policy is the view's alone.
#[derive(Debug)]
pub(super) struct TabDrag<T> {
    pub(super) tab: Option<T>,
    pub(super) target: Option<DropTarget>,
}

/// Hand-written because a derive would demand `T: Default`; the row's default is "no drag".
impl<T> Default for TabDrag<T> {
    fn default() -> Self {
        Self {
            tab: None,
            target: None,
        }
    }
}
