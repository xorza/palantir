//! [`UserEvent`]: what a [`HostHandle`](crate::HostHandle) sends a running
//! [`WinitHost`](crate::WinitHost)'s event loop, and the tasks it carries.

use std::fmt;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::SyncSender;

use crate::driver::error::PlayError;
use crate::gpu::device::gpu::Gpu;
use crate::host::winit::owned_input_event::OwnedInputEvent;
use crate::host::winit::system_preferences::SystemReport;
use crate::host::winit::wake_latch::WakeLatch;
use crate::primitives::geometry::rect::Rect;
use crate::ui::Ui;
use crate::window::screenshot::Screenshot;
use crate::window::window_token::WindowToken;

/// A main-thread closure scheduled via [`HostHandle::run_on_main`](crate::HostHandle::run_on_main).
pub(super) type MainTask<T> = Box<dyn FnOnce(&mut T) + Send>;

/// Events delivered through [`HostHandle`](crate::HostHandle) to the event loop. Generic over `T`
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

/// A closure the event loop runs between two frames of one window, with `&mut` the app and the
/// window's `Ui`.
pub(crate) type QueryTask<T> = Box<dyn FnOnce(&mut T, &mut Ui) + Send>;

/// A closure the event loop runs between two frames of one window, with the app and the window's
/// `Ui`, which asks no frame.
pub(crate) type ReadTask<T> = Box<dyn FnOnce(&T, &Ui) + Send>;

/// A closure the event loop runs between two frames of one window, with the device the host paints
/// with.
pub(crate) type LoseTask = Box<dyn FnOnce(&Gpu) + Send>;

/// A screenshot a remote asked of a window, and where its image goes.
#[derive(Debug)]
pub(crate) struct ShotAsk {
    pub(super) region: Option<Rect>,
    pub(super) asked: Asked<Result<Screenshot, PlayError>>,
}

/// An answer a task owes the remote that sent it, unless that remote gave up waiting.
#[derive(Debug)]
pub(super) struct Asked<R> {
    pub(super) reply: SyncSender<R>,
    /// Read before `f` runs: a channel tells its sender the receiver left only through a send,
    /// which comes after `f`.
    pub(super) given_up: Arc<AtomicBool>,
}

impl<R> Asked<R> {
    /// Answer with what `f` returns; a task given up on runs nothing.
    pub(super) fn answer(self, f: impl FnOnce() -> R) {
        if !self.is_given_up() {
            self.reply.send(f()).ok();
        }
    }

    /// Whether the remote that asked stopped waiting for the answer.
    pub(super) fn is_given_up(&self) -> bool {
        self.given_up.load(Ordering::Acquire)
    }
}

#[cfg(test)]
mod tests {
    use crate::host::winit::user_event::UserEvent;
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
