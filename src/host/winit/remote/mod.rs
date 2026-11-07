//! [`Remote`]: a live window of a running [`WinitHost`](crate::WinitHost), played from another
//! thread as a person at it would, with the actions, locators and reads a
//! [`Harness`](crate::Harness) takes.

pub(crate) mod remote_end;

use std::fmt::{self, Debug};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender};
use std::thread;
use std::time::{Duration, Instant};

use glam::Vec2;

use crate::driver::action::Action;
use crate::driver::agent_window::AgentWindow;
use crate::driver::error::{ActError, PlayError};
use crate::driver::locator::{Found, Locate, Locator, Reach};
use crate::driver::play::{self, BUSY_SETTLE_WAITS, Stage};
use crate::host::winit::frame_summary::FrameSummary;
use crate::host::winit::handle::{HostHandle, UserEvent};
use crate::host::winit::owned_input_event::OwnedInputEvent;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::input::input_event::InputEvent;
use crate::ui::Ui;
use crate::ui::inspected_widget::InspectedWidget;
use crate::window::screenshot::Screenshot;
use crate::window::window_token::WindowToken;

/// The longest real time a remote waits for the window by default: for an answer, a frame, or a
/// settle.
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
    pub(super) region: Option<Rect>,
    pub(super) reply: SyncSender<Result<Screenshot, PlayError>>,
}

/// One window of a running [`WinitHost`](crate::WinitHost), driven from another thread: queries
/// run between two of its frames with `&mut` the app and its `Ui`, input goes in as the window's
/// OS input does, and every wait is bounded in real time. Made by [`HostHandle::remote`].
///
/// Its waits read the window's frame summaries, counting the inputs and queries this remote sent:
/// input other senders send meanwhile can make a step settle early.
pub struct Remote<T: 'static> {
    handle: HostHandle<T>,
    window: WindowToken,
    /// Whether something outside the host holds the window's thread, as a native dialog does.
    blocked: Option<Box<dyn Fn() -> bool + Send>>,
    /// The events sent, inputs and queries, which a frame that saw them all counts.
    sent: u64,
    /// The last press's real time, which the next press waits out the double-click interval from.
    last_press: Option<Instant>,
    /// The longest real time a wait for the window lasts: [`WAIT`] unless [`Self::with_wait`]
    /// sets it.
    wait: Duration,
    /// How often a wait checks whether it may go on: [`TICK`], shorter in tests.
    tick: Duration,
}

impl<T: 'static> Debug for Remote<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Remote")
            .field("window", &self.window)
            .field("sent", &self.sent)
            .finish_non_exhaustive()
    }
}

impl<T: 'static> AgentWindow<T> for Remote<T> {
    /// Wait for a frame that saw every event this remote sent, asks no repaint and finds the app
    /// not [busy](crate::App::is_busy), as a [`HarnessSession`](crate::HarnessSession) waits on
    /// its clock; returns the frames the window ran meanwhile.
    ///
    /// # Errors
    ///
    /// [`PlayError::Unsettled`] when the window still asks for frames after the
    /// [wait](Self::with_wait), [`PlayError::Busy`] when a busy app runs no frame for as long,
    /// [`PlayError::NoFrame`] when the window runs none, [`PlayError::Blocked`] and
    /// [`PlayError::Hidden`] at once when it cannot, and [`PlayError::Disconnected`] when it
    /// closed.
    fn settle(&mut self) -> Result<u32, PlayError> {
        let first = self.record_frame_id();
        self.wait_frame(Until::Settled)?;
        Ok(self.frames_since(first))
    }

    /// Play `actions` in order as a person at the window would, in real time, settling after
    /// each, as a [`HarnessSession`](crate::HarnessSession) plays them. Returns the frames the window
    /// ran meanwhile.
    ///
    /// # Errors
    ///
    /// [`ActError`], naming the step it stopped at.
    fn play<X: Locate<T> + Clone + Send + 'static>(
        &mut self,
        actions: &[Action<X>],
    ) -> Result<u32, ActError> {
        let first = self.record_frame_id();
        let mut stage = RemoteStage { remote: self };
        for (ran, action) in actions.iter().enumerate() {
            play::play(&mut stage, action).map_err(|error| ActError { ran, error })?;
        }
        Ok(self.frames_since(first))
    }

    /// A picture of `region`, the whole window for none, as a later frame paints it.
    ///
    /// # Errors
    ///
    /// [`PlayError::EmptyRegion`] for a region outside the window, [`PlayError::Disconnected`]
    /// when the window closed, [`PlayError::Blocked`] and [`PlayError::Hidden`] at once when no
    /// frame can run, and [`PlayError::NoFrame`] when no picture landed within the
    /// [wait](Self::with_wait).
    fn shot(&mut self, region: Option<Rect>) -> Result<Screenshot, PlayError> {
        self.check_waiting(Instant::now(), Answer::InFrame)?;
        let (reply, answer) = mpsc::sync_channel(1);
        self.handle
            .send(UserEvent::Shot(self.window, ShotAsk { region, reply }))
            .map_err(|_| PlayError::Disconnected)?;
        self.receive(&answer, Answer::InFrame)?
    }

    /// The one place `at` names in the last frame, as a [`HarnessSession`](crate::HarnessSession)
    /// finds it.
    ///
    /// # Errors
    ///
    /// [`PlayError::Locate`] when `at` names no one place, and as [`Self::query`].
    fn find<X: Locate<T> + Clone + Send + 'static>(
        &mut self,
        at: &Locator<X>,
    ) -> Result<Found, PlayError> {
        let at = at.clone();
        Ok(self.query(move |app, ui| at.find(&*app, ui))??)
    }

    /// The visible interactive widgets of the last frame whose text holds `filter`, ignoring
    /// case, in paint order, as a [`HarnessSession`](crate::HarnessSession) lists them.
    ///
    /// # Errors
    ///
    /// As [`Self::query`].
    fn widgets(&mut self, filter: Option<&str>) -> Result<Vec<InspectedWidget>, PlayError> {
        let filter = filter.map(str::to_owned);
        self.query(move |_, ui| InspectedWidget::visible(ui, filter.as_deref()))
    }
}

impl<T: 'static> Remote<T> {
    pub(crate) fn new(handle: HostHandle<T>, window: WindowToken) -> Self {
        let sent = handle
            .last_frame(window)
            .map_or(0, |summary| summary.sent_events);
        Self {
            handle,
            window,
            blocked: None,
            sent,
            last_press: None,
            wait: WAIT,
            tick: TICK,
        }
    }

    /// Give up every wait at once with [`PlayError::Blocked`] while `blocked` says something
    /// outside the host holds the window's thread, as a native file dialog does until the person
    /// answers it: no event and no frame runs then. Each wait asks it as it polls.
    #[must_use]
    pub fn with_blocked(mut self, blocked: impl Fn() -> bool + Send + 'static) -> Self {
        self.blocked = Some(Box::new(blocked));
        self
    }

    /// Wait for the window at most `wait` of real time, for an answer, a frame or a settle; 30 s
    /// by default.
    #[must_use]
    pub const fn with_wait(mut self, wait: Duration) -> Self {
        self.wait = wait;
        self
    }

    /// Run `f` between two frames of the window with `&mut` the app and the window's `Ui`, which
    /// holds the last frame; a frame that records follows, which the next [settle](Self::settle)
    /// waits for, so a read after it sees what `f` changed; a hidden window runs that frame when
    /// it shows, so a settle meanwhile ends with [`PlayError::Hidden`]. Returns what `f`
    /// returns. A query given up on never runs later, unless it had begun. A panic in `f` is the
    /// window's: it ends the process under `panic = "abort"`, as any panic on the window's thread
    /// does.
    ///
    /// # Errors
    ///
    /// [`PlayError::Disconnected`] when the window closed or the loop ended,
    /// [`PlayError::Blocked`] at once while its thread is held, and [`PlayError::NoFrame`] when no
    /// answer came within the [wait](Self::with_wait).
    pub fn query<R: Send + 'static>(
        &mut self,
        f: impl FnOnce(&mut T, &mut Ui) -> R + Send + 'static,
    ) -> Result<R, PlayError> {
        // Before sending, so a held thread gives up at once and the query never runs.
        self.check_waiting(Instant::now(), Answer::BetweenFrames)?;
        let (reply, answer) = mpsc::sync_channel(1);
        let given_up = Arc::new(AtomicBool::new(false));
        let skip = Arc::clone(&given_up);
        let task: QueryTask<T> = Box::new(move |app, ui| {
            if !skip.load(Ordering::Acquire) {
                reply.send(f(app, ui)).ok();
            }
        });
        self.handle
            .send(UserEvent::Query(self.window, task))
            .map_err(|_| PlayError::Disconnected)?;
        self.sent += 1;
        let answered = self.receive(&answer, Answer::BetweenFrames);
        if answered.is_err() {
            given_up.store(true, Ordering::Release);
        }
        answered
    }

    /// What `answer` receives, bounded in real time; an answer a frame gives waits on a hidden
    /// window, whose frames wait for it to show, not at all.
    fn receive<R>(&self, answer: &Receiver<R>, comes: Answer) -> Result<R, PlayError> {
        let start = Instant::now();
        loop {
            match answer.recv_timeout(self.tick) {
                Ok(value) => return Ok(value),
                Err(RecvTimeoutError::Disconnected) => return Err(PlayError::Disconnected),
                Err(RecvTimeoutError::Timeout) => self.check_waiting(start, comes)?,
            }
        }
    }

    /// Whether a wait that began at `start` may go on: its thread not held, the window shown, for
    /// an answer a frame gives, and not past the remote's wait.
    fn check_waiting(&self, start: Instant, comes: Answer) -> Result<(), PlayError> {
        if self.blocked.as_ref().is_some_and(|blocked| blocked()) {
            return Err(PlayError::Blocked);
        }
        if comes == Answer::InFrame && self.handle.is_hidden(self.window) {
            return Err(PlayError::Hidden);
        }
        let waited = start.elapsed();
        if waited > self.wait {
            return Err(PlayError::NoFrame { waited });
        }
        Ok(())
    }

    fn last_frame(&self) -> Result<FrameSummary, PlayError> {
        self.handle
            .last_frame(self.window)
            .ok_or(PlayError::Disconnected)
    }

    fn record_frame_id(&self) -> u64 {
        self.handle
            .last_frame(self.window)
            .map_or(0, |summary| summary.record_frame_id)
    }

    /// The frames recorded since `first`.
    fn frames_since(&self, first: u64) -> u32 {
        u32::try_from(self.record_frame_id().saturating_sub(first)).unwrap_or(u32::MAX)
    }

    /// Wait for a frame `until` names. A busy app's wait for its next frame starts again at each,
    /// as its work goes on.
    fn wait_frame(&self, until: Until) -> Result<(), PlayError> {
        let settled = until == Until::Settled;
        let start = Instant::now();
        let first = self.record_frame_id();
        let mut seen = first;
        let mut progressed = start;
        loop {
            let last = self.last_frame()?;
            let unsettled = settled && (last.repaint_requested || last.busy);
            if last.sent_events >= self.sent && !unsettled {
                return Ok(());
            }
            let working = settled && last.busy && !last.repaint_requested;
            if last.record_frame_id != seen {
                seen = last.record_frame_id;
                progressed = Instant::now();
            }
            let since = if working { progressed } else { start };
            if working && start.elapsed() > self.wait.saturating_mul(BUSY_SETTLE_WAITS) {
                return Err(PlayError::StillBusy {
                    waited: start.elapsed(),
                });
            }
            if let Err(error) = self.check_waiting(since, Answer::InFrame) {
                let frames = self.frames_since(first);
                return Err(match error {
                    PlayError::NoFrame { waited } if working => PlayError::Busy { waited },
                    PlayError::NoFrame { .. } if settled && frames > 0 => {
                        PlayError::Unsettled { frames }
                    }
                    error => error,
                });
            }
            self.handle.frame_log.wait(self.tick);
        }
    }
}

/// Which frame a remote waits for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Until {
    /// One that saw every event sent.
    SawEvents,
    /// One that saw every event sent, asks no repaint and finds the app not busy.
    Settled,
}

/// Where an answer a remote waits for comes from, which decides whether a hidden window gives it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Answer {
    /// A query, which the loop runs between frames, also of a hidden window.
    BetweenFrames,
    /// A frame, which a hidden window does not run.
    InFrame,
}

/// A remote as the [`Stage`] the shared rules play on, in real time.
struct RemoteStage<'a, T: 'static> {
    remote: &'a mut Remote<T>,
}

impl<T: 'static, X: Locate<T> + Clone + Send + 'static> Stage<X> for RemoteStage<'_, T> {
    fn point(&mut self, at: &Locator<X>, reach: Reach) -> Result<Vec2, PlayError> {
        let at = at.clone();
        Ok(self
            .remote
            .query(move |app, ui| at.point_for(&*app, ui, reach))??)
    }

    fn input(&mut self, event: InputEvent<'_>) {
        self.remote
            .handle
            .send_input(self.remote.window, OwnedInputEvent::from(event));
        self.remote.sent += 1;
    }

    fn frame(&mut self) -> Result<(), PlayError> {
        self.remote.wait_frame(Until::SawEvents)
    }

    fn settle(&mut self) -> Result<(), PlayError> {
        self.remote.wait_frame(Until::Settled)
    }

    fn wait(&mut self, dt: Duration) -> Result<(), PlayError> {
        thread::sleep(dt);
        Ok(())
    }

    fn space_press(&mut self) -> Result<(), PlayError> {
        let Some(pressed) = self.remote.last_press else {
            return Ok(());
        };
        let interval = self.remote.query(|_, ui| ui.double_click_interval())?;
        let since = pressed.elapsed();
        if since <= interval {
            thread::sleep(interval - since + Duration::from_millis(1));
        }
        Ok(())
    }

    fn note_press(&mut self) {
        self.remote.last_press = Some(Instant::now());
    }
}

#[cfg(test)]
pub(crate) mod internals {
    use crate::host::winit::remote::Remote;
    use std::time::Duration;

    impl<T: 'static> Remote<T> {
        /// Check each wait every `tick`, so a test that outlasts one waits a few milliseconds.
        pub(crate) const fn with_tick(mut self, tick: Duration) -> Self {
            self.tick = tick;
            self
        }
    }
}

#[cfg(test)]
mod tests;
