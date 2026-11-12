//! [`HostHandle`] and [`UserEvent`]: the cross-thread channel into a running [`WinitHost`](super::WinitHost).

use std::fmt;
use std::sync::Arc;
#[cfg(test)]
use std::sync::mpsc;

use winit::event_loop::EventLoopProxy;

use crate::host::winit::error::HostDisconnected;
use crate::host::winit::frame_log::FrameLog;
use crate::host::winit::frame_summary::FrameSummary;
use crate::host::winit::owned_input_event::OwnedInputEvent;
use crate::host::winit::remote::{LoseTask, QueryTask, ReadTask, Remote, ShotAsk};
use crate::host::winit::system_preferences::SystemReport;
use crate::host::winit::wake_latch::WakeLatch;
use crate::window::window_token::WindowToken;

/// A main-thread closure scheduled via [`HostHandle::run_on_main`].
pub(super) type MainTask<T> = Box<dyn FnOnce(&mut T) + Send>;

/// Events delivered through [`HostHandle`] to the event loop. Generic over `T`
/// so [`Self::RunOnMain`] carries a typed closure. Window lifecycle is an
/// in-frame action ([`Ui::open_window`](crate::Ui::open_window)), so a
/// background thread wanting a window pokes a `Repaint`.
pub(crate) enum UserEvent<T> {
    /// Wakes the loop for one redraw of the named window; one in flight a window, which the
    /// latch holds.
    Repaint(WindowToken, WakeLatch),
    /// Input for the named window, delivered as its OS input is.
    Input(WindowToken, OwnedInputEvent),
    /// Runs a closure on the main thread with `&mut` the app, then every window follows what it
    /// changed.
    RunOnMain(MainTask<T>),
    /// Runs a closure with `&mut` the app and the named window's `Ui` between two of its frames,
    /// then runs a frame; dropped unrun when the window is gone.
    Query(WindowToken, QueryTask<T>),
    /// Runs a closure with the app and the named window's `Ui` between two of its frames, and runs
    /// no frame; dropped unrun when the window is gone.
    Read(WindowToken, ReadTask<T>),
    /// Asks the named window for a screenshot, which its frame hands back when it lands.
    Shot(WindowToken, ShotAsk),
    /// Runs a closure with the host's device between two of the named window's frames, which
    /// loses it, then runs a frame, which opens a new one; dropped unrun when the window is gone.
    LoseDevice(WindowToken, LoseTask),
    /// The system's settings moved, as one of their sources reports from its thread.
    System(SystemReport),
    /// The system warns of low memory, as its watch reports from its thread.
    MemoryPressure,
    /// Asks the event loop to exit.
    Quit,
}

impl<T> fmt::Debug for UserEvent<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Repaint(token, _) => f
                .debug_tuple("Repaint")
                .field(token)
                .finish_non_exhaustive(),
            Self::Input(token, event) => f.debug_tuple("Input").field(token).field(event).finish(),
            Self::RunOnMain(_) => f.write_str("RunOnMain(..)"),
            Self::Query(token, _) => f.debug_tuple("Query").field(token).finish_non_exhaustive(),
            Self::Read(token, _) => f.debug_tuple("Read").field(token).finish_non_exhaustive(),
            Self::Shot(token, ask) => f.debug_tuple("Shot").field(token).field(ask).finish(),
            Self::LoseDevice(token, _) => f
                .debug_tuple("LoseDevice")
                .field(token)
                .finish_non_exhaustive(),
            Self::System(report) => f.debug_tuple("System").field(report).finish(),
            Self::MemoryPressure => f.write_str("MemoryPressure"),
            Self::Quit => f.write_str("Quit"),
        }
    }
}

/// Thread-safe, cheaply `Clone` handle to a running
/// [`WinitHost<T>`](super::WinitHost); obtain one via
/// [`WinitHost::handle`](super::WinitHost::handle) before `run`.
pub struct HostHandle<T: 'static> {
    pub(super) sender: EventSender<T>,
    pub(super) frame_log: Arc<FrameLog>,
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
            frame_log: Arc::clone(&self.frame_log),
        }
    }
}

impl<T: 'static> fmt::Debug for HostHandle<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HostHandle").finish_non_exhaustive()
    }
}

impl<T: 'static> HostHandle<T> {
    /// Requests one frame of `window`, behind the window's [`Waker`](crate::Waker), the one
    /// cross-thread wake an app holds, unless `latch` holds one in flight. Lock-free; dropped
    /// silently if the loop or window is gone.
    pub(crate) fn request_repaint(&self, window: WindowToken, latch: &WakeLatch) {
        if latch.raise() {
            self.send(UserEvent::Repaint(window, latch.clone())).ok();
        }
    }

    /// Delivers `event` to `window` before its next frame, as the window's OS input is, stamped
    /// with the window's input clock, then runs a frame; [`FrameSummary::sent_events`] counts the
    /// event. Events reach the window in the order they are sent. Dropped silently unless the
    /// window is open. Crate-private, so a [`Remote`], which counts what it sends,
    /// is the one sender: input from another made its settle end early.
    pub(crate) fn send_input(&self, window: WindowToken, event: OwnedInputEvent) {
        self.send(UserEvent::Input(window, event)).ok();
    }

    /// The last frame `window` ran, [`FrameSummary::AT_OPEN`] until it runs one, or `None` when it
    /// is not open: before it opens, after it closes, and once the loop ends. A window skips its
    /// frames while hidden, so its summary holds still.
    pub(crate) fn last_frame(&self, window: WindowToken) -> Option<FrameSummary> {
        self.frame_log.get(window)
    }

    /// Whether `window` opened and has not closed, its first frame still to come included; `false`
    /// once the loop ends.
    pub fn is_open(&self, window: WindowToken) -> bool {
        self.last_frame(window).is_some()
    }

    /// Whether `window` is minimized or occluded, which holds its frames until it shows; `false`
    /// once it closes and once the loop ends.
    pub fn is_hidden(&self, window: WindowToken) -> bool {
        self.frame_log.is_hidden(window)
    }

    /// Schedules `f` on the main thread with `&mut` the app before the next
    /// frame, and repaints every window after it, so what `f` changed shows.
    ///
    /// # Errors
    ///
    /// [`HostDisconnected`] when the event loop has exited; `f` is dropped
    /// unrun. This is the only method that reports delivery, as the only one
    /// whose loss discards a state mutation.
    pub fn run_on_main(
        &self,
        f: impl FnOnce(&mut T) + Send + 'static,
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
    use crate::host::winit::wake_latch::WakeLatch;
    use crate::primitives::input::input_event::InputEvent;
    use crate::window::window_token::WindowToken;

    #[test]
    fn user_event_debug_formats_every_variant_without_an_app_bound() {
        let repaint: UserEvent<()> = UserEvent::Repaint(WindowToken(7), WakeLatch::default());
        let task = UserEvent::RunOnMain(Box::new(|(): &mut ()| {}));
        let quit: UserEvent<()> = UserEvent::Quit;
        let input: UserEvent<()> =
            UserEvent::Input(WindowToken(2), InputEvent::ImeCommit("ok").into());

        assert_eq!(format!("{repaint:?}"), "Repaint(WindowToken(7), ..)");
        assert_eq!(format!("{task:?}"), "RunOnMain(..)");
        assert_eq!(format!("{quit:?}"), "Quit");
        assert_eq!(
            format!("{input:?}"),
            r#"Input(WindowToken(2), OwnedInputEvent { event: ImeCommit(""), text: "ok" })"#
        );
    }
}
