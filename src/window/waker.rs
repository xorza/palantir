//! [`Waker`]: a handle any thread holds to ask a window for a frame.

use std::fmt;
use std::sync::Arc;

/// Asks the window whose [`Ui::waker`](crate::Ui::waker) made it for a frame, from any thread,
/// for work done off the frame: a worker that finished, a file that loaded. Every host gives
/// one. The winit host runs the window's next frame. [`Harness`](crate::Harness) marks a wake
/// that its next frame takes as a repaint request, and that [`Harness::settle`] waits for while
/// the app [is busy](crate::App::is_busy). [`OffscreenHost`](crate::OffscreenHost) runs no loop
/// of its own, so its next frame takes the wake as a repaint request.
///
/// [`Harness::settle`]: crate::Harness::settle
#[derive(Clone)]
pub struct Waker {
    wake: Arc<dyn Fn() + Send + Sync>,
}

impl Waker {
    /// A waker that calls `wake`.
    pub(crate) fn new(wake: impl Fn() + Send + Sync + 'static) -> Self {
        Self {
            wake: Arc::new(wake),
        }
    }

    /// Ask the window for a frame. Many wakes before that frame run it once.
    pub fn wake(&self) {
        (self.wake)();
    }
}

impl fmt::Debug for Waker {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Waker").finish_non_exhaustive()
    }
}
