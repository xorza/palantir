//! [`HostHandle`] + [`UserEvent`] — the cross-thread poke channel into a
//! running [`WinitHost`](super::WinitHost). Background threads hold a
//! `HostHandle<T>` and send `UserEvent<T>`s through the event-loop proxy
//! to request a redraw, run a closure on the main thread with `&mut` the
//! app, or ask the loop to exit.

use winit::event_loop::EventLoopProxy;

use crate::window::WindowToken;

/// A main-thread closure scheduled via [`HostHandle::run_on_main`],
/// invoked with `&mut` the host's app `T`.
pub(crate) type MainTask<T> = Box<dyn FnOnce(&mut T) -> bool + Send>;

/// Events delivered to the host through [`HostHandle`] — cross-thread
/// pokes the winit event loop turns into a redraw of a window, a
/// run-on-main callback, or an exit. Generic over the host's app type
/// `T` so [`Self::RunOnMain`] carries a typed `&mut T` closure with no
/// downcast. Public only as the type parameter of `EventLoopProxy`;
/// construct via the methods on [`HostHandle`].
///
/// There is no `OpenWindow` / `CloseWindow` variant: window lifecycle is
/// an in-frame UI action ([`Ui::open_window`](crate::Ui::open_window)),
/// not an off-thread one — a background thread that wants a new window
/// pokes a `Repaint` and lets the next `frame` call `open_window`.
pub enum UserEvent<T> {
    /// Wake the loop and request one redraw of the named window.
    /// Coalesced — many in a row collapse to one frame.
    Repaint(WindowToken),
    /// Run a closure on the main (event-loop) thread with `&mut` the
    /// app, then repaint every window if it returns `true`.
    RunOnMain(MainTask<T>),
    /// Ask the event loop to exit at the next opportunity.
    Quit,
}

impl<T> std::fmt::Debug for UserEvent<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Repaint(token) => f.debug_tuple("Repaint").field(token).finish(),
            Self::RunOnMain(_) => f.write_str("RunOnMain(..)"),
            Self::Quit => f.write_str("Quit"),
        }
    }
}

/// Thread-safe handle to a running [`WinitHost<T>`](super::WinitHost).
/// Cheaply `Clone`; send to background threads so they can poke the UI
/// without owning it. `T` is the host's app type — only
/// [`Self::run_on_main`] actually uses it.
///
/// Obtain one via [`WinitHost::handle`](super::WinitHost::handle) before
/// calling `run`.
pub struct HostHandle<T: 'static> {
    pub(crate) proxy: EventLoopProxy<UserEvent<T>>,
}

// Hand-written so the impls don't pick up a spurious `T: Clone` / `T:
// Debug` bound — the handle stores only a proxy, never a `T`. (`T:
// 'static` is unavoidable: the `EventLoopProxy<UserEvent<T>>` field
// requires it.)
impl<T: 'static> Clone for HostHandle<T> {
    fn clone(&self) -> Self {
        Self {
            proxy: self.proxy.clone(),
        }
    }
}

impl<T: 'static> std::fmt::Debug for HostHandle<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HostHandle").finish_non_exhaustive()
    }
}

impl<T: 'static> HostHandle<T> {
    /// Request the host paint one frame of the window named by `win`.
    /// Cheap and lock-free; safe to call from any thread. Drops silently
    /// if the event loop has already exited or the window is gone.
    pub fn request_repaint(&self, win: WindowToken) {
        let _ = self.proxy.send_event(UserEvent::Repaint(win));
    }

    /// Schedule `f` to run on the main (event-loop) thread with `&mut`
    /// access to the app before the next frame — the safe way to fold
    /// background-thread results into app state without a separate
    /// channel. Return `true` from `f` to repaint every window, `false`
    /// to leave the present schedule unchanged.
    pub fn run_on_main(&self, f: impl FnOnce(&mut T) -> bool + Send + 'static) {
        let _ = self.proxy.send_event(UserEvent::RunOnMain(Box::new(f)));
    }

    /// Ask the host's event loop to exit. The current frame finishes;
    /// no further frames are scheduled.
    pub fn quit(&self) {
        let _ = self.proxy.send_event(UserEvent::Quit);
    }
}

#[cfg(test)]
mod tests {
    use crate::host::winit::handle::UserEvent;
    use crate::window::WindowToken;

    #[test]
    fn user_event_debug_formats_every_variant_without_an_app_bound() {
        let repaint: UserEvent<()> = UserEvent::Repaint(WindowToken(7));
        let task = UserEvent::RunOnMain(Box::new(|_: &mut ()| true));
        let quit: UserEvent<()> = UserEvent::Quit;

        assert_eq!(format!("{repaint:?}"), "Repaint(WindowToken(7))");
        assert_eq!(format!("{task:?}"), "RunOnMain(..)");
        assert_eq!(format!("{quit:?}"), "Quit");
    }
}
