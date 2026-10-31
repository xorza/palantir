//! [`HostHandle`] and [`UserEvent`]: the cross-thread channel into a running [`WinitHost`](super::WinitHost).

use std::fmt;
use std::sync::Arc;
#[cfg(test)]
use std::sync::mpsc;

use winit::event_loop::EventLoopProxy;

use crate::host::winit::error::HostDisconnected;
use crate::host::winit::frame_log::FrameLog;
use crate::host::winit::frame_summary::FrameSummary;
use crate::host::winit::remote::{QueryTask, Remote, ShotAsk};
use crate::host::winit::system_preferences::SystemPreferences;
use crate::input::owned_input_event::OwnedInputEvent;
use crate::window::window_token::WindowToken;

/// A main-thread closure scheduled via [`HostHandle::run_on_main`].
pub(super) type MainTask<T> = Box<dyn FnOnce(&mut T) -> bool + Send>;

/// Events delivered through [`HostHandle`] to the event loop. Generic over `T`
/// so [`Self::RunOnMain`] carries a typed closure. Window lifecycle is an
/// in-frame action ([`Ui::open_window`](crate::Ui::open_window)), so a
/// background thread wanting a window pokes a `Repaint`.
pub(crate) enum UserEvent<T> {
    /// Wakes the loop for one redraw of the named window; coalesced.
    Repaint(WindowToken),
    /// Input for the named window, delivered as its OS input is.
    Input(WindowToken, OwnedInputEvent),
    /// Runs a closure on the main thread with `&mut` the app, then repaints every window if it returns `true`.
    RunOnMain(MainTask<T>),
    /// Runs a closure with `&mut` the app and the named window's `Ui` between two of its frames,
    /// then runs a frame; dropped unrun when the window is gone.
    Query(WindowToken, QueryTask<T>),
    /// Asks the named window for a screenshot, which its frame hands back when it lands.
    Shot(WindowToken, ShotAsk),
    /// The system's settings moved, as `mundy`'s subscription reports them from its thread.
    SystemPreferences(SystemPreferences),
    /// Asks the event loop to exit.
    Quit,
}

impl<T> fmt::Debug for UserEvent<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Repaint(token) => f.debug_tuple("Repaint").field(token).finish(),
            Self::Input(token, event) => f.debug_tuple("Input").field(token).field(event).finish(),
            Self::RunOnMain(_) => f.write_str("RunOnMain(..)"),
            Self::Query(token, _) => f.debug_tuple("Query").field(token).finish_non_exhaustive(),
            Self::Shot(token, ask) => f.debug_tuple("Shot").field(token).field(ask).finish(),
            Self::SystemPreferences(preferences) => f
                .debug_tuple("SystemPreferences")
                .field(preferences)
                .finish(),
            Self::Quit => f.write_str("Quit"),
        }
    }
}

/// Thread-safe, cheaply `Clone` handle to a running
/// [`WinitHost<T>`](super::WinitHost); obtain one via
/// [`WinitHost::handle`](super::WinitHost::handle) before `run`.
pub struct HostHandle<T: 'static> {
    pub(super) sender: EventSender<T>,
    pub(super) frames: Arc<FrameLog>,
}

/// Where a handle's events go: the event loop, or in this crate's tests a channel a loop of
/// their own serves.
pub(super) enum EventSender<T: 'static> {
    Proxy(EventLoopProxy<UserEvent<T>>),
    #[cfg(test)]
    Channel(mpsc::Sender<UserEvent<T>>),
}

impl<T: 'static> EventSender<T> {
    fn send(&self, event: UserEvent<T>) -> Result<(), HostDisconnected> {
        match self {
            Self::Proxy(proxy) => proxy.send_event(event).map_err(|_| HostDisconnected),
            #[cfg(test)]
            Self::Channel(sender) => sender.send(event).map_err(|_| HostDisconnected),
        }
    }
}

// Hand-written to avoid a spurious `T: Clone`/`T: Debug` bound; the handle stores no `T`.
impl<T: 'static> Clone for HostHandle<T> {
    fn clone(&self) -> Self {
        let sender = match &self.sender {
            EventSender::Proxy(proxy) => EventSender::Proxy(proxy.clone()),
            #[cfg(test)]
            EventSender::Channel(sender) => EventSender::Channel(sender.clone()),
        };
        Self {
            sender,
            frames: Arc::clone(&self.frames),
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
        self.send(UserEvent::Repaint(window)).ok();
    }

    /// Delivers `event` to `window` before its next frame, as the window's OS input is, stamped
    /// with the window's input clock, then runs a frame; [`FrameSummary::sent_inputs`] counts the
    /// event. Events reach the window in the order they are sent. Dropped silently unless the
    /// window is open.
    pub fn send_input(&self, window: WindowToken, event: OwnedInputEvent) {
        self.send(UserEvent::Input(window, event)).ok();
    }

    /// The last frame `window` ran, or `None` before its first frame, after it closes, and
    /// once the loop ends. A window skips its frames while hidden, so its summary holds still.
    pub fn last_frame(&self, window: WindowToken) -> Option<FrameSummary> {
        self.frames.get(window)
    }

    /// Whether `window` is minimized or occluded, which holds its frames until it shows; `false`
    /// once it closes and once the loop ends.
    pub fn is_hidden(&self, window: WindowToken) -> bool {
        self.frames.is_hidden(window)
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
        self.send(UserEvent::RunOnMain(Box::new(f)))
    }

    /// Asks the host to close every window and then end the event loop; the current frame
    /// finishes. The windows leave the screen before the app drops.
    pub fn quit(&self) {
        self.send(UserEvent::Quit).ok();
    }

    /// The window `window` of this host, played and read from this thread.
    pub fn remote(&self, window: WindowToken) -> Remote<T> {
        Remote::new(self.clone(), window)
    }

    pub(crate) fn send(&self, event: UserEvent<T>) -> Result<(), HostDisconnected> {
        self.sender.send(event)
    }
}

#[cfg(test)]
mod tests {
    use crate::host::winit::handle::UserEvent;
    use crate::input::input_event::InputEvent;
    use crate::window::window_token::WindowToken;

    #[test]
    fn user_event_debug_formats_every_variant_without_an_app_bound() {
        let repaint: UserEvent<()> = UserEvent::Repaint(WindowToken(7));
        let task = UserEvent::RunOnMain(Box::new(|(): &mut ()| true));
        let quit: UserEvent<()> = UserEvent::Quit;
        let input: UserEvent<()> =
            UserEvent::Input(WindowToken(2), InputEvent::ImeCommit("ok").into());

        assert_eq!(format!("{repaint:?}"), "Repaint(WindowToken(7))");
        assert_eq!(format!("{task:?}"), "RunOnMain(..)");
        assert_eq!(format!("{quit:?}"), "Quit");
        assert_eq!(
            format!("{input:?}"),
            r#"Input(WindowToken(2), OwnedInputEvent { event: ImeCommit(""), text: "ok" })"#
        );
    }
}
