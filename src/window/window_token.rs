//! The app's own identity for one window.

/// Caller-chosen opaque identity for a window. Supplied at
/// [`Ui::open_window`](crate::Ui::open_window) (and
/// [`WinitHost::builder`](crate::WinitHost::builder) for a host's bootstrap
/// window; the offscreen host has one fixed window, so its token is the
/// constant [`OffscreenHost::WINDOW`](crate::OffscreenHost::WINDOW)),
/// handed back to [`App::update`](crate::App::update) and
/// [`App::record`](crate::App::record), and used
/// to address a window in [`Ui::close_window`](crate::Ui::close_window) /
/// [`HostHandle::request_repaint`](crate::HostHandle::request_repaint).
/// The app owns the semantics — use it as an enum discriminant, an index,
/// a document-id hash, whatever. Palantir only stores and compares it;
/// winit's `WindowId` never reaches the app.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct WindowToken(pub u64);
