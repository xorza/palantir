//! [`Harness::play`], [`Harness::settle`] and [`Harness::shot`]: the window driven as a person at
//! it would, waiting for it to settle after each step.

use crate::app::App;
use crate::driver::action::Action;
use crate::driver::error::{ActError, PlayError};
use crate::driver::locator::{Locate, Locator};
use crate::driver::play::{self, Stage};
use crate::host::harness::Harness;
use crate::host::window_driver::FrameDepth;
use crate::input::input_event::InputEvent;
use crate::primitives::geometry::rect::Rect;
use crate::ui::frame_report::FrameReport;
use crate::window::screenshot::Screenshot;
use glam::Vec2;
use std::fmt::Debug;
use std::time::{Duration, Instant};

impl Harness {
    /// Play `actions` on `app` in order, as a person at the window would, settling after each;
    /// the first that fails stops the rest. Every locator of a step resolves before its input
    /// goes in, and a button or a modifier it holds is always released, so a failed step leaves
    /// nothing held. Separate presses wait out the double-click window, and a press moves the
    /// pointer first and settles, so it lands on what the pointer hovers. Returns the frames run.
    ///
    /// # Errors
    ///
    /// [`ActError`], naming the step it stopped at.
    pub fn play<A: App, X: Locate<A>>(
        &mut self,
        app: &mut A,
        actions: &[Action<X>],
    ) -> Result<u32, ActError> {
        let start = self.frames;
        let mut stage = HarnessStage { h: self, app };
        for (ran, action) in actions.iter().enumerate() {
            play::play(&mut stage, action).map_err(|error| ActError { ran, error })?;
        }
        Ok(self.frames - start)
    }

    /// Run frames of `app`, the clock moving one frame's time before each, until one asks no
    /// repaint, no screenshot is on its way back from the device, which it waits for, no
    /// [wake](crate::Waker) waits, and the app is not [busy](App::is_busy). While the app is busy
    /// and asks nothing, the settle waits in real time for the wake its work sends. A caret blink
    /// asks a timed wake, not a repaint, so it settles. Returns the frames run.
    ///
    /// # Errors
    ///
    /// [`PlayError::Unsettled`] when the window still asks after the builder's
    /// [`settle_frames`](crate::HarnessBuilder::settle_frames), and [`PlayError::Busy`] when a
    /// busy app sends no wake in its
    /// [`background_wait`](crate::HarnessBuilder::background_wait).
    pub fn settle(&mut self, app: &mut impl App) -> Result<u32, PlayError> {
        self.settle_after(app, &format_args!("the settle"))
    }

    /// Block until a [`Waker`](crate::Waker) of this window wakes it, or `deadline` passes, in
    /// real time; whether a wake waits for the next frame. For a caller that waits on work it
    /// knows of, past what [`Self::settle`] waits for.
    pub fn wait_for_wake(&self, deadline: Instant) -> bool {
        self.wake.wait(deadline)
    }

    /// Whether a [`Waker`](crate::Waker) of this window woke it since the last frame, which the
    /// next frame takes.
    pub fn has_pending_wake(&self) -> bool {
        self.wake.is_pending()
    }

    /// A picture of `region`, the whole window for none, as the next frame paints it, as the
    /// app's own [`Ui::request_screenshot`](crate::Ui::request_screenshot) takes it.
    ///
    /// # Errors
    ///
    /// [`PlayError::NoPixels`] below [`Depth::Gpu`](crate::Depth::Gpu),
    /// [`PlayError::EmptyRegion`] for a region outside the window, and
    /// [`PlayError::Unsettled`] when the picture has not landed after the builder's
    /// [`settle_frames`](crate::HarnessBuilder::settle_frames).
    pub fn shot(
        &mut self,
        app: &mut impl App,
        region: Option<Rect>,
    ) -> Result<Screenshot, PlayError> {
        if self.depth != FrameDepth::Paint {
            return Err(PlayError::NoPixels);
        }
        let asked = self
            .driver
            .ui
            .request_screenshot(region)
            .ok_or(PlayError::EmptyRegion)?;
        // The frame paints the image; once the device's copy is back, the next frame lands it,
        // after any image an earlier ask is owed, one a frame.
        self.next_frame(app);
        for _ in 0..self.settle_frames {
            self.wait_for_gpu();
            self.next_frame(app);
            if let Some(shot) = self.driver.ui.screenshot_taken(asked) {
                return Ok(shot.clone());
            }
        }
        Err(PlayError::Unsettled {
            step: format!("the shot of {region:?}"),
            frames: self.settle_frames,
        })
    }

    /// Frames of `app` until it settles, as [`Self::settle`] runs them, naming `step` when it
    /// does not.
    fn settle_after(&mut self, app: &mut impl App, step: &dyn Debug) -> Result<u32, PlayError> {
        let start = self.frames;
        let deadline = Instant::now() + self.background_wait;
        for _ in 0..self.settle_frames {
            let report = self.next_frame(app);
            if self.driver.has_pending_screenshots() {
                self.wait_for_gpu();
                continue;
            }
            if report.repaint_requested || self.wake.is_pending() {
                continue;
            }
            if !app.is_busy() {
                return Ok(self.frames - start);
            }
            if !self.wake.wait(deadline) {
                return Err(PlayError::Busy {
                    step: format!("{step:?}"),
                    seconds: self.background_wait.as_secs_f32(),
                });
            }
        }
        Err(PlayError::Unsettled {
            step: format!("{step:?}"),
            frames: self.settle_frames,
        })
    }

    /// One frame of `app`, one frame's time after the last.
    fn next_frame(&mut self, app: &mut impl App) -> FrameReport {
        self.advance(self.frame_dt);
        self.frame_app(app)
    }
}

/// A harness and the app it plays, as a [`Stage`] on the harness's stepped clock.
struct HarnessStage<'a, A> {
    h: &'a mut Harness,
    app: &'a mut A,
}

impl<A: App, X: Locate<A>> Stage<X> for HarnessStage<'_, A> {
    fn point(&mut self, at: &Locator<X>) -> Result<Vec2, PlayError> {
        Ok(at.find(self.app, &self.h.driver.ui)?.point)
    }

    /// A modifier change that holds what is held already sends nothing, which would wait a frame.
    fn input(&mut self, event: InputEvent<'_>) {
        match event {
            InputEvent::ModifiersChanged(mods) => self.h.set_modifiers(mods),
            event => {
                self.h.on_input(event);
            }
        }
    }

    fn frame(&mut self) -> Result<(), PlayError> {
        self.h.next_frame(self.app);
        Ok(())
    }

    fn settle(&mut self, step: &dyn Debug) -> Result<(), PlayError> {
        self.h.settle_after(self.app, step).map(drop)
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
        let window = self.h.driver.ui.double_click_window();
        let since = self.h.time.saturating_sub(pressed);
        if since <= window {
            self.h.advance(window - since + Duration::from_millis(1));
        }
        Ok(())
    }

    fn note_press(&mut self) -> Result<(), PlayError> {
        self.h.last_press = Some(self.h.time);
        Ok(())
    }
}
