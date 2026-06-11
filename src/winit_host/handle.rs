//! [`HostHandle`] + [`UserEvent`] — the cross-thread poke channel into a
//! running [`WinitHost`](super::WinitHost). Background threads hold a
//! `HostHandle` and send `UserEvent`s through the event-loop proxy to
//! request a redraw or run a closure on the main thread.

use winit::event_loop::EventLoopProxy;

use crate::ui::Ui;
use crate::window::WindowToken;

pub(crate) type MainTask = Box<dyn FnOnce(&mut Ui) -> bool + Send>;

/// Events delivered to the host through [`HostHandle`] — cross-thread
/// pokes that the winit event loop turns into a redraw or a run-on-main
/// callback against a specific window. Public only as the type parameter
/// of `EventLoopProxy`; construct via the methods on [`HostHandle`].
///
/// Note there is no `OpenWindow` / `CloseWindow` variant: window
/// lifecycle is an in-frame UI action ([`Ui::open_window`]), not an
/// off-thread one — a background thread that wants a new window pokes a
/// `Repaint` and lets the next `frame` call `open_window`.
pub enum UserEvent {
    /// Wake the loop and request one redraw of the named window.
    /// Coalesced — many in a row collapse to one frame.
    Repaint(WindowToken),
    /// Run a closure on the main (event-loop) thread with the named
    /// window's `&mut Ui`, then request a redraw.
    RunOnMain(WindowToken, MainTask),
    /// Ask the event loop to exit at the next opportunity.
    Quit,
}

/// Thread-safe handle to a running [`WinitHost`](super::WinitHost).
/// Cheaply `Clone`; send to background threads so they can poke the UI
/// without owning it.
///
/// Obtain one via [`WinitHost::handle`](super::WinitHost::handle) before
/// calling `run`.
#[derive(Clone, Debug)]
pub struct HostHandle {
    pub(crate) proxy: EventLoopProxy<UserEvent>,
}

impl HostHandle {
    /// Request the host paint one frame of the window named by `win`.
    /// Cheap and lock-free; safe to call from any thread. Drops silently
    /// if the event loop has already exited or the window is gone.
    pub fn request_repaint(&self, win: WindowToken) {
        let _ = self.proxy.send_event(UserEvent::Repaint(win));
    }

    /// Schedule `f` to run on the main thread with the `win` window's
    /// `&mut Ui` before the next frame. Use for state mutations that
    /// aren't safe to perform off-thread (touching the recorder, the
    /// wgpu device, etc.). Return `true` from `f` to request a repaint,
    /// `false` to leave the present schedule unchanged. `f` may call
    /// `ui.open_window(..)` — the request drains on the next
    /// `about_to_wait`.
    pub fn run_on_main(&self, win: WindowToken, f: impl FnOnce(&mut Ui) -> bool + Send + 'static) {
        let _ = self
            .proxy
            .send_event(UserEvent::RunOnMain(win, Box::new(f)));
    }

    /// Ask the host's event loop to exit. The current frame finishes;
    /// no further frames are scheduled.
    pub fn quit(&self) {
        let _ = self.proxy.send_event(UserEvent::Quit);
    }
}
