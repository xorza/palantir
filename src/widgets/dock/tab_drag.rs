//! The tab a pointer is carrying.

/// A tab mid-drag, kept on the dock's own [`Ui`](crate::Ui) state row
/// rather than in application state: the gesture belongs to the widget,
/// and nothing outside it can act on a half-finished drag.
///
/// Nothing here is positional. `tab` is an identity, so an undo that
/// rearranges a strip mid-drag cannot strand the gesture on a slot the
/// tab has left. The ghost chip's label is re-read from
/// [`DockTabs::title`](crate::DockTabs::title) on the frame it is
/// painted, so no snapshot of it goes stale either.
#[derive(Debug)]
pub(crate) struct TabDrag<T> {
    pub(crate) tab: Option<T>,
}

/// Hand-written rather than derived: a derive would demand `T: Default`,
/// and a tab key is an application enum with no meaningful default. The
/// row's own default is "no drag", which needs nothing of `T`.
impl<T> Default for TabDrag<T> {
    fn default() -> Self {
        Self { tab: None }
    }
}
