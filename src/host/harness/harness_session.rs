//! [`HarnessSession`]: a [`Harness`] and its app, driven as an [`AgentWindow`].

use crate::driver::action::Action;
use crate::driver::agent_window::AgentWindow;
use crate::driver::error::{PlayError, StepError};
use crate::driver::locator::{Locate, Locator, Reach};
use crate::driver::play::{self, Stage};
use crate::host::harness::{Harness, device_answered};
use crate::primitives::geometry::rect::Rect;
use crate::primitives::input::input_event::InputEvent;
use crate::ui::Ui;
use crate::ui::app::App;
use crate::window::screenshot::Screenshot;
use glam::Vec2;
use std::time::Duration;

/// A [`Harness`] with the app it runs, from [`Harness::session`]: the [`AgentWindow`] verbs, as a
/// [`Remote`](crate::Remote) offers them on a live window. Borrowed for as long as the steps take,
/// as the harness owns no app.
#[derive(Debug)]
pub struct HarnessSession<'h, A> {
    pub(super) harness: &'h mut Harness,
    pub(super) app: &'h mut A,
}

impl<A: App> AgentWindow<A> for HarnessSession<'_, A> {
    /// Play `actions` on the app in order, as a person at the window would, settling after each;
    /// the first that fails stops the rest. Every locator of a step resolves before its input
    /// goes in, and a button or a modifier it holds is always released, so a failed step leaves
    /// nothing held. Separate presses wait out the double-click interval, and a press moves the
    /// pointer first and settles, so it lands on what the pointer hovers. Returns the frames
    /// recorded.
    ///
    /// # Errors
    ///
    /// [`StepError`], naming the step it stopped at.
    fn play<X: Locate<A>>(&mut self, actions: &[Action<X>]) -> Result<u32, StepError> {
        let first = self.harness.ui().record_frame_id();
        let mut stage = HarnessStage {
            h: self.harness,
            app: self.app,
        };
        play::play_all(&mut stage, actions)?;
        Ok(play::frames_recorded(
            first,
            self.harness.ui().record_frame_id(),
        ))
    }

    /// Run frames of the app, the clock moving one frame's time before each, until one asks no
    /// repaint, no screenshot is on its way back from the device, which it waits for, no
    /// [wake](crate::Waker) waits, and the app is not [busy](App::is_busy). While the app is busy
    /// and asks nothing, the settle waits in real time for each wake its work sends, as long as
    /// the work goes on. A caret blink asks a timed wake, not a repaint, so it settles. Returns
    /// the frames recorded.
    ///
    /// # Errors
    ///
    /// [`PlayError::Unsettled`] when the window still asks after the builder's
    /// [`settle_frames`](crate::HarnessBuilder::settle_frames) frames that ask,
    /// [`PlayError::Busy`] when a busy app sends no wake in its
    /// [`background_wait`](crate::HarnessBuilder::background_wait), [`PlayError::StillBusy`] when
    /// it keeps waking the window past twenty of those waits, [`PlayError::DeviceLost`] when the
    /// window asks for frames or screenshots of a lost device, and [`PlayError::GpuTimedOut`] when
    /// the device does not finish the copy of a screenshot it asks for.
    fn settle(&mut self) -> Result<u32, PlayError> {
        self.harness.settle_frames_of(self.app)
    }

    /// A picture of `region`, the whole window for none, as the next frame paints it, as the
    /// app's own [`Ui::request_screenshot`](crate::Ui::request_screenshot) takes it.
    ///
    /// # Errors
    ///
    /// [`PlayError::NoPixels`] below [`Depth::Gpu`](crate::Depth::Gpu),
    /// [`PlayError::EmptyRegion`] for a region outside the window, [`PlayError::DeviceLost`] when
    /// the device is lost, [`PlayError::GpuTimedOut`] when it does not finish the frame in time,
    /// and [`PlayError::Unsettled`] when the picture has not landed after the builder's
    /// [`settle_frames`](crate::HarnessBuilder::settle_frames).
    fn shot(&mut self, region: Option<Rect>) -> Result<Screenshot, PlayError> {
        let h = &mut *self.harness;
        if !h.paint.paints() {
            return Err(PlayError::NoPixels);
        }
        let asked = h
            .ui_mut()
            .request_screenshot(region)
            .ok_or(PlayError::EmptyRegion)?;
        // The frame paints the image; once the device's copy is back, the next frame lands it,
        // after any image an earlier ask is owed, one a frame.
        h.next_frame(self.app);
        for _ in 0..h.pace.settle_frames {
            device_answered(h.wait_for_gpu())?;
            h.next_frame(self.app);
            if let Some(shot) = h.ui().screenshot_taken(asked) {
                return Ok(shot.clone());
            }
        }
        // The last frame's submit can find the loss.
        if h.is_device_lost() {
            return Err(PlayError::DeviceLost);
        }
        Err(PlayError::Unsettled {
            frames: h.pace.settle_frames,
        })
    }

    /// `f` on the app and the last frame, at once: no frame runs and the clock holds still.
    fn read<R: Send + 'static>(
        &mut self,
        f: impl FnOnce(&A, &Ui) -> R + Send + 'static,
    ) -> Result<R, PlayError> {
        Ok(f(self.app, self.harness.ui()))
    }

    /// `f` on the app and the last frame, at once, and the next frame records, as `f` may have
    /// changed the app.
    fn query<R: Send + 'static>(
        &mut self,
        f: impl FnOnce(&mut A, &mut Ui) -> R + Send + 'static,
    ) -> Result<R, PlayError> {
        let ui = self.harness.ui_mut();
        let answer = f(self.app, ui);
        ui.request_repaint();
        Ok(answer)
    }

    /// Lose the harness's device at once. The frames after it run the app and paint nothing until
    /// [`Harness::recover`].
    ///
    /// # Errors
    ///
    /// [`PlayError::NoPixels`] below [`Depth::Gpu`](crate::Depth::Gpu).
    fn lose_device(&mut self) -> Result<(), PlayError> {
        self.harness.gpu().ok_or(PlayError::NoPixels)?.lose();
        Ok(())
    }
}

/// A harness and the app it plays, as a [`Stage`] on the harness's stepped clock.
struct HarnessStage<'a, A> {
    h: &'a mut Harness,
    app: &'a mut A,
}

impl<A: App, X: Locate<A>> Stage<X> for HarnessStage<'_, A> {
    fn point(&mut self, at: &Locator<X>, reach: Reach) -> Result<Vec2, PlayError> {
        Ok(at.point_for(self.app, self.h.ui(), reach)?)
    }

    fn input(&mut self, event: InputEvent<'_>) {
        self.h.on_input(event);
    }

    fn frame(&mut self) -> Result<(), PlayError> {
        self.h.next_frame(self.app);
        Ok(())
    }

    fn settle(&mut self) -> Result<(), PlayError> {
        self.h.settle_frames_of(self.app).map(drop)
    }

    fn wait(&mut self, dt: Duration) -> Result<(), PlayError> {
        let end = self.h.time + dt;
        while self.h.time < end {
            self.h.next_frame(self.app);
        }
        Ok(())
    }

    fn space_press(&mut self) -> Result<(), PlayError> {
        let Some(pressed) = self.h.last_press else {
            return Ok(());
        };
        let interval = self.h.ui().double_click_interval();
        let since = self.h.time.saturating_sub(pressed);
        if since <= interval {
            self.h.advance(interval - since + Duration::from_millis(1));
        }
        Ok(())
    }

    fn note_press(&mut self) {
        self.h.last_press = Some(self.h.time);
    }
}
