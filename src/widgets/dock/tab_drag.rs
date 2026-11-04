//! The tab a pointer is carrying, and where the last frame showed it would land.

use crate::widgets::dock::pane_geometry::DropTarget;

/// A tab mid-drag, kept on the dock's own [`Ui`](crate::Ui) state row since only the widget can act on a half-finished drag. The row exists only while a tab is carried, so no drag holds a drop with no tab.
///
/// `tab` is an identity, so an undo rearranging the strip can't strand the gesture, and the ghost label is re-read from the tab's label at paint. `target` is the drop the last frame classified under the view's split policy and showed: the scan that sees the release lands the tab there, as the policy is the view's alone.
#[derive(Clone, Copy, Debug)]
pub(super) struct TabDrag<T> {
    pub(super) tab: T,
    pub(super) target: Option<DropTarget>,
}
