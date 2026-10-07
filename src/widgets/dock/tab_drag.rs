//! The tab a pointer is carrying.

/// A tab mid-drag, kept on the dock's own [`Ui`](crate::Ui) state row since only the widget can act on a half-finished drag.
///
/// Nothing here is positional: `tab` is an identity, so an undo rearranging the strip can't strand the gesture, and the ghost label is re-read from [`DockTabs::title`](crate::DockTabs::title) at paint.
#[derive(Debug)]
pub(crate) struct TabDrag<T> {
    pub(crate) tab: Option<T>,
}

/// Hand-written because a derive would demand `T: Default`; the row's default is "no drag".
impl<T> Default for TabDrag<T> {
    fn default() -> Self {
        Self { tab: None }
    }
}
