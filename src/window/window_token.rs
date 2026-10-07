//! The app's own identity for one window.

/// Caller-chosen opaque identity for a window, given at [`Ui::open_window`](crate::Ui::open_window) or [`WinitHost::builder`](crate::WinitHost::builder) and handed back to [`App::update`](crate::App::update) and [`App::record`](crate::App::record); Palantir only stores and compares it.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct WindowToken(pub u64);
