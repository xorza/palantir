//! [`HostHandle`] and [`UserEvent`]: the cross-thread channel into a running [`WinitHost`](super::WinitHost).

use winit::event_loop::EventLoopProxy;

use crate::host::winit::error::HostDisconnected;
use crate::window::window_token::WindowToken;
use std::fmt;

/// A main-thread closure scheduled via [`HostHandle::run_on_main`].
pub(super) type MainTask<T> = Box<dyn FnOnce(&mut T) -> bool + Send>;

/// Events delivered through [`HostHandle`] to the event loop. Generic over `T`
/// so [`Self::RunOnMain`] carries a typed closure. Window lifecycle is an
/// in-frame action ([`Ui::open_window`](crate::Ui::open_window)), so a
/// background thread wanting a window pokes a `Repaint`.
pub(crate) enum UserEvent<T> {
    /// Wakes the loop for one redraw of the named window; coalesced.
    Repaint(WindowToken),
    /// Runs a closure on the main thread with `&mut` the app, then repaints every window if it returns `true`.
    RunOnMain(MainTask<T>),
    /// Asks the event loop to exit.
    Quit,
}

impl<T> fmt::Debug for UserEvent<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Repaint(token) => f.debug_tuple("Repaint").field(token).finish(),
            Self::RunOnMain(_) => f.write_str("RunOnMain(..)"),
            Self::Quit => f.write_str("Quit"),
        }
    }
}

/// Thread-safe, cheaply `Clone` handle to a running
/// [`WinitHost<T>`](super::WinitHost); obtain one via
/// [`WinitHost::handle`](super::WinitHost::handle) before `run`.
pub struct HostHandle<T: 'static> {
    pub(super) proxy: EventLoopProxy<UserEvent<T>>,
}

// Hand-written to avoid a spurious `T: Clone`/`T: Debug` bound; the handle stores only a proxy.
impl<T: 'static> Clone for HostHandle<T> {
    fn clone(&self) -> Self {
        Self {
            proxy: self.proxy.clone(),
        }
    }
}

impl<T: 'static> fmt::Debug for HostHandle<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HostHandle").finish_non_exhaustive()
    }
}

impl<T: 'static> HostHandle<T> {
    /// Requests one frame of `window`. Lock-free; dropped silently if the loop or window is gone.
    pub fn request_repaint(&self, window: WindowToken) {
        let _ = self.proxy.send_event(UserEvent::Repaint(window));
    }

    /// Schedules `f` on the main thread with `&mut` the app before the next
    /// frame; return `true` to repaint every window.
    ///
    /// # Errors
    ///
    /// [`HostDisconnected`] when the event loop has exited; `f` is dropped
    /// unrun. This is the only method that reports delivery, as the only one
    /// whose loss discards a state mutation.
    pub fn run_on_main(
        &self,
        f: impl FnOnce(&mut T) -> bool + Send + 'static,
    ) -> Result<(), HostDisconnected> {
        self.proxy
            .send_event(UserEvent::RunOnMain(Box::new(f)))
            .map_err(|_| HostDisconnected)
    }

    /// Asks the event loop to exit; the current frame finishes.
    pub fn quit(&self) {
        let _ = self.proxy.send_event(UserEvent::Quit);
    }
}

#[cfg(test)]
mod tests {
    use crate::host::winit::handle::UserEvent;
    use crate::window::window_token::WindowToken;

    #[test]
    fn user_event_debug_formats_every_variant_without_an_app_bound() {
        let repaint: UserEvent<()> = UserEvent::Repaint(WindowToken(7));
        let task = UserEvent::RunOnMain(Box::new(|(): &mut ()| true));
        let quit: UserEvent<()> = UserEvent::Quit;

        assert_eq!(format!("{repaint:?}"), "Repaint(WindowToken(7))");
        assert_eq!(format!("{task:?}"), "RunOnMain(..)");
        assert_eq!(format!("{quit:?}"), "Quit");
    }
}
