//! [`HostHandle`]: the cross-thread channel into a running [`WinitHost`](super::WinitHost), which sends it [`UserEvent`]s.

use std::fmt;
use std::sync::Arc;
#[cfg(test)]
use std::sync::mpsc;

use winit::event_loop::EventLoopProxy;

use crate::host::winit::error::HostHandleError;
use crate::host::winit::frame_log::{FrameLog, LoggedFrame};
use crate::host::winit::owned_input_event::OwnedInputEvent;
use crate::host::winit::remote::Remote;
use crate::host::winit::user_event::UserEvent;
use crate::host::winit::wake_latch::WakeLatch;
use crate::window::window_token::WindowToken;

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
    fn send(&self, event: UserEvent<T>) -> Result<(), HostHandleError> {
        match self {
            Self::Proxy(proxy) => proxy.send_event(event).map_err(|_| HostHandleError),
            #[cfg(test)]
            Self::Channel(sender) => sender.send(event).map_err(|_| HostHandleError),
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
    /// with the window's input clock, then runs a frame; the summary's
    /// [`sent_events`](crate::host::winit::frame_summary::FrameSummary::sent_events) counts the
    /// event. Events reach the window in the order they are sent. Dropped silently unless the
    /// window is open. Crate-private, so a [`Remote`], which counts what it sends, is the one
    /// sender: input from another made its settle end early.
    pub(crate) fn send_input(&self, window: WindowToken, event: OwnedInputEvent) {
        self.send(UserEvent::Input(window, event)).ok();
    }

    /// The last frame `window` ran,
    /// [`FrameSummary::AT_OPEN`](crate::host::winit::frame_summary::FrameSummary::AT_OPEN) until it
    /// runs one, with the opening that ran it, or `None` when it is not open: before it opens,
    /// after it closes, and once the loop ends. A window skips its frames while hidden, so its
    /// summary holds still.
    pub(crate) fn last_frame(&self, window: WindowToken) -> Option<LoggedFrame> {
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
    /// [`HostHandleError`] when the event loop has exited; `f` is dropped
    /// unrun. This is the only method that reports delivery, as the only one
    /// whose loss discards a state mutation.
    pub fn run_on_main(
        &self,
        f: impl FnOnce(&mut T) + Send + 'static,
    ) -> Result<(), HostHandleError> {
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

    pub(crate) fn send(&self, event: UserEvent<T>) -> Result<(), HostHandleError> {
        self.sender.send(event)
    }
}
