//! [`Remote`]: a live window of a running [`WinitHost`](crate::WinitHost), played from another
//! thread as a person at it would, with the actions, locators and reads a
//! [`Harness`](crate::Harness) takes.

use std::any::Any;
use std::cell::Cell;
use std::fmt::{self, Debug};
use std::panic::{self, AssertUnwindSafe};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender};
use std::thread;
use std::time::{Duration, Instant};

use glam::Vec2;

use crate::driver::action::Action;
use crate::driver::error::{ActError, PlayError};
use crate::driver::locator::{Locate, Locator};
use crate::driver::play::{self, Stage};
use crate::host::winit::frame_summary::FrameSummary;
use crate::host::winit::handle::{HostHandle, UserEvent};
use crate::input::input_event::InputEvent;
use crate::input::owned_input_event::OwnedInputEvent;
use crate::primitives::geometry::rect::Rect;
use crate::ui::Ui;
use crate::ui::inspected_widget::InspectedWidget;
use crate::window::screenshot::{Screenshot, ScreenshotId};
use crate::window::window_token::WindowToken;

/// The longest real time a remote waits for the window: for an answer, a frame, or a settle.
const WAIT: Duration = Duration::from_secs(30);

/// How often a wait checks whether the window's thread is held, between the frames and answers
/// that end it at once.
const TICK: Duration = Duration::from_millis(50);

/// A closure the event loop runs between two frames of one window, with `&mut` the app and the
/// window's `Ui`.
pub(crate) type QueryTask<T> = Box<dyn FnOnce(&mut T, &mut Ui) + Send>;

/// A screenshot a remote asked of a window, and where its image goes.
#[derive(Debug)]
pub(crate) struct ShotAsk {
    region: Option<Rect>,
    reply: SyncSender<Result<Screenshot, PlayError>>,
}

/// The screenshots remotes asked of one window that have not landed. Each answers in the frame
/// its image lands in, which reads it before a later frame replaces it; a window that closes
/// drops the asks, which its remotes read as [`PlayError::Disconnected`].
#[derive(Debug, Default)]
pub(crate) struct RemoteShots {
    pending: Vec<PendingShot>,
}

#[derive(Debug)]
struct PendingShot {
    id: ScreenshotId,
    reply: SyncSender<Result<Screenshot, PlayError>>,
}

impl RemoteShots {
    /// Ask `ui` for the screenshot `ask` names; a region with no pixel of the window answers at
    /// once.
    pub(crate) fn ask(&mut self, ui: &mut Ui, ask: ShotAsk) {
        match ui.request_screenshot(ask.region) {
            Some(id) => self.pending.push(PendingShot {
                id,
                reply: ask.reply,
            }),
            None => {
                ask.reply.send(Err(PlayError::EmptyRegion)).ok();
            }
        }
    }

    /// Hand each screenshot that landed in the frame `ui` just ran to the remote that asked.
    pub(crate) fn deliver(&mut self, ui: &Ui) {
        self.pending
            .retain(|pending| match ui.screenshot_taken(pending.id) {
                Some(shot) => {
                    pending.reply.send(Ok(shot.clone())).ok();
                    false
                }
                None => true,
            });
    }
}

/// One window of a running [`WinitHost`](crate::WinitHost), driven from another thread: queries
/// run between two of its frames with `&mut` the app and its `Ui`, input goes in as the window's
/// OS input does, and every wait is bounded in real time. Made by [`HostHandle::remote`].
///
/// Its waits read the window's frame summaries, counting the input this remote sent: input other
/// senders send meanwhile can make a step settle early.
pub struct Remote<T: 'static> {
    handle: HostHandle<T>,
    window: WindowToken,
    /// Whether something outside the host holds the window's thread, as a native dialog does.
    blocked: Option<Box<dyn Fn() -> bool + Send>>,
    /// The events sent, which a frame that saw them all counts.
    sent: Cell<u64>,
    /// The last press's real time, which the next press waits out the double-click window from.
    last_press: Cell<Option<Instant>>,
}

impl<T: 'static> Debug for Remote<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Remote")
            .field("window", &self.window)
            .field("sent", &self.sent.get())
            .finish_non_exhaustive()
    }
}

impl<T: 'static> Remote<T> {
    pub(crate) fn new(handle: HostHandle<T>, window: WindowToken) -> Self {
        let sent = handle
            .last_frame(window)
            .map_or(0, |summary| summary.sent_inputs);
        Self {
            handle,
            window,
            blocked: None,
            sent: Cell::new(sent),
            last_press: Cell::new(None),
        }
    }

    /// Give up every wait at once while `blocked` says something outside the host holds the
    /// window's thread, as a native file dialog does until the person answers it: no event and
    /// no frame runs then. Each wait asks it as it polls.
    #[must_use]
    pub fn with_blocked(mut self, blocked: impl Fn() -> bool + Send + 'static) -> Self {
        self.blocked = Some(Box::new(blocked));
        self
    }

    /// Run `f` between two frames of the window with `&mut` the app and the window's `Ui`, which
    /// holds the last frame; a frame follows. Returns what `f` returns. A query given up on never
    /// runs later, unless it had begun.
    ///
    /// # Errors
    ///
    /// [`PlayError::Disconnected`] when the window closed or the loop ended,
    /// [`PlayError::NoFrame`] when no answer came in 30 s, and [`PlayError::Panicked`] when `f`
    /// panicked: the panic ends the query alone, and the window goes on.
    pub fn query<R: Send + 'static>(
        &self,
        f: impl FnOnce(&mut T, &mut Ui) -> R + Send + 'static,
    ) -> Result<R, PlayError> {
        let (reply, answer) = mpsc::sync_channel(1);
        let given_up = Arc::new(AtomicBool::new(false));
        let skip = Arc::clone(&given_up);
        let task: QueryTask<T> = Box::new(move |app, ui| {
            if !skip.load(Ordering::Acquire) {
                let answer =
                    panic::catch_unwind(AssertUnwindSafe(|| f(app, ui))).map_err(|payload| {
                        PlayError::Panicked {
                            message: panic_message(&*payload).to_owned(),
                        }
                    });
                reply.send(answer).ok();
            }
        });
        self.handle
            .send(UserEvent::Query(self.window, task))
            .map_err(|_| PlayError::Disconnected)?;
        let answered = self.receive(&answer);
        if answered.is_err() {
            given_up.store(true, Ordering::Release);
        }
        answered?
    }

    /// Wait for a frame that saw every event this remote sent, asks no repaint and finds the app
    /// not [busy](crate::App::is_busy), as [`Harness::settle`](crate::Harness::settle) waits on
    /// its clock; returns the frames the window ran meanwhile.
    ///
    /// # Errors
    ///
    /// [`PlayError::Unsettled`] when the window still asks for frames after 30 s,
    /// [`PlayError::Busy`] when the app stays busy as long, [`PlayError::NoFrame`] when it runs
    /// none, and [`PlayError::Disconnected`] when it closed.
    pub fn settle(&self) -> Result<u32, PlayError> {
        let first = self.frame_id();
        self.wait_frame(true, &"the settle")?;
        Ok(u32::try_from(self.frame_id().saturating_sub(first)).unwrap_or(u32::MAX))
    }

    /// Play `actions` in order as a person at the window would, in real time, settling after
    /// each, as [`Harness::play`](crate::Harness::play) plays them. Returns the frames the window
    /// ran meanwhile.
    ///
    /// # Errors
    ///
    /// [`ActError`], naming the step it stopped at.
    pub fn play<X: Locate<T> + Clone + Send + 'static>(
        &self,
        actions: &[Action<X>],
    ) -> Result<u32, ActError> {
        let first = self.frame_id();
        let mut stage = RemoteStage { remote: self };
        for (ran, action) in actions.iter().enumerate() {
            play::play(&mut stage, action).map_err(|error| ActError { ran, error })?;
        }
        let frames = self.frame_id().saturating_sub(first);
        Ok(u32::try_from(frames).unwrap_or(u32::MAX))
    }

    /// A picture of `region`, the whole window for none, as a later frame paints it.
    ///
    /// # Errors
    ///
    /// [`PlayError::EmptyRegion`] for a region outside the window, [`PlayError::Disconnected`]
    /// when the window closed, and [`PlayError::NoFrame`] when no picture landed in 30 s.
    pub fn shot(&self, region: Option<Rect>) -> Result<Screenshot, PlayError> {
        let (reply, answer) = mpsc::sync_channel(1);
        self.handle
            .send(UserEvent::Shot(self.window, ShotAsk { region, reply }))
            .map_err(|_| PlayError::Disconnected)?;
        self.receive(&answer)?
    }

    /// The visible interactive widgets of the last frame whose text holds `filter`, ignoring
    /// case, in paint order, as [`Harness::widgets`](crate::Harness::widgets) lists them.
    ///
    /// # Errors
    ///
    /// As [`Self::query`].
    pub fn widgets(&self, filter: Option<&str>) -> Result<Vec<InspectedWidget>, PlayError> {
        let filter = filter.map(str::to_owned);
        self.query(move |_, ui| InspectedWidget::visible(ui, filter.as_deref()))
    }

    /// What `answer` receives, bounded in real time; a hidden window, whose frames wait for it to
    /// show, gives up at once.
    fn receive<R>(&self, answer: &Receiver<R>) -> Result<R, PlayError> {
        let start = Instant::now();
        loop {
            match answer.recv_timeout(TICK) {
                Ok(value) => return Ok(value),
                Err(RecvTimeoutError::Disconnected) => return Err(PlayError::Disconnected),
                Err(RecvTimeoutError::Timeout) => self.check_waiting(start)?,
            }
        }
    }

    /// Whether a wait that began at `start` may go on: not past [`WAIT`], the window shown, and
    /// its thread not held.
    fn check_waiting(&self, start: Instant) -> Result<(), PlayError> {
        let waited = start.elapsed();
        let blocked = self.blocked.as_ref().is_some_and(|blocked| blocked());
        if waited > WAIT || blocked || self.handle.is_hidden(self.window) {
            return Err(PlayError::NoFrame {
                seconds: waited.as_secs_f32(),
            });
        }
        Ok(())
    }

    fn last_frame(&self) -> Result<FrameSummary, PlayError> {
        self.handle
            .last_frame(self.window)
            .ok_or(PlayError::Disconnected)
    }

    fn frame_id(&self) -> u64 {
        self.handle
            .last_frame(self.window)
            .map_or(0, |summary| summary.frame_id)
    }

    /// Wait for a frame that saw every event sent and, when `settled`, asks no repaint and finds
    /// the app not busy.
    fn wait_frame(&self, settled: bool, step: &dyn Debug) -> Result<(), PlayError> {
        let start = Instant::now();
        let first = self.frame_id();
        loop {
            let last = self.last_frame()?;
            let unsettled = settled && (last.repaint_requested || last.busy);
            if last.sent_inputs >= self.sent.get() && !unsettled {
                return Ok(());
            }
            if let Err(error) = self.check_waiting(start) {
                let frames = last.frame_id.saturating_sub(first);
                let waited = start.elapsed();
                return Err(
                    if settled && waited > WAIT && last.busy && !last.repaint_requested {
                        PlayError::Busy {
                            step: format!("{step:?}"),
                            seconds: waited.as_secs_f32(),
                        }
                    } else if settled && frames > 0 && waited > WAIT {
                        PlayError::Unsettled {
                            step: format!("{step:?}"),
                            frames: u32::try_from(frames).unwrap_or(u32::MAX),
                        }
                    } else {
                        error
                    },
                );
            }
            self.handle.frames.wait(TICK);
        }
    }
}

/// A remote as the [`Stage`] the shared rules play on, in real time.
struct RemoteStage<'a, T: 'static> {
    remote: &'a Remote<T>,
}

impl<T: 'static, X: Locate<T> + Clone + Send + 'static> Stage<X> for RemoteStage<'_, T> {
    fn point(&mut self, at: &Locator<X>) -> Result<Vec2, PlayError> {
        let at = at.clone();
        Ok(self.remote.query(move |app, ui| at.find(&*app, ui))??.point)
    }

    fn input(&mut self, event: InputEvent<'_>) {
        let remote = self.remote;
        remote
            .handle
            .send_input(remote.window, OwnedInputEvent::from(event));
        remote.sent.set(remote.sent.get() + 1);
    }

    fn frame(&mut self) -> Result<(), PlayError> {
        self.remote.wait_frame(false, &"a frame")
    }

    fn settle(&mut self, step: &dyn Debug) -> Result<(), PlayError> {
        self.remote.wait_frame(true, step)
    }

    fn wait(&mut self, dt: Duration) -> Result<(), PlayError> {
        thread::sleep(dt);
        Ok(())
    }

    fn space_press(&mut self) -> Result<(), PlayError> {
        let Some(pressed) = self.remote.last_press.get() else {
            return Ok(());
        };
        let window = self.remote.query(|_, ui| ui.double_click_window())?;
        let since = pressed.elapsed();
        if since <= window {
            thread::sleep(window - since + Duration::from_millis(1));
        }
        Ok(())
    }

    fn note_press(&mut self) -> Result<(), PlayError> {
        self.remote.last_press.set(Some(Instant::now()));
        Ok(())
    }
}

/// A panic payload's message: the `String` or `&str` a `panic!` carries.
fn panic_message(payload: &(dyn Any + Send)) -> &str {
    payload
        .downcast_ref::<String>()
        .map(String::as_str)
        .or_else(|| payload.downcast_ref::<&str>().copied())
        .unwrap_or("a panic with no message")
}

#[cfg(test)]
mod tests;
