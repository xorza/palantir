//! The harness side of [`HarnessSession`](crate::HarnessSession)'s play, settle and shot: the
//! window driven as a person at it would, waiting for it to settle after each step.

use crate::driver::action::Action;
use crate::driver::error::{ActError, PlayError};
use crate::driver::locator::{Locate, Locator, Reach};
use crate::driver::play::{self, BUSY_SETTLE_WAITS, Stage};
use crate::gpu::device::gpu_wait::GpuWait;
use crate::host::harness::Harness;
use crate::input::input_event::InputEvent;
use crate::primitives::geometry::rect::Rect;
use crate::ui::app::App;
use crate::ui::frame_report::FrameReport;
use crate::window::screenshot::Screenshot;
use glam::Vec2;
use std::time::{Duration, Instant};

impl Harness {
    /// [`HarnessSession`](crate::HarnessSession)'s play of `actions` on `app`.
    pub(super) fn play<A: App, X: Locate<A>>(
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

    /// Block until a [`Waker`](crate::Waker) of this window wakes it, or `timeout` passes, in
    /// real time; whether a wake waits for the next frame. For a caller that waits on work it
    /// knows of, past what [a session's settle](crate::AgentWindow::settle) waits for.
    pub fn wait_for_wake(&self, timeout: Duration) -> bool {
        self.window.wait_for_wake(Instant::now() + timeout)
    }

    /// Whether a [`Waker`](crate::Waker) of this window woke it since the last frame, which the
    /// next frame takes.
    pub fn has_pending_wake(&self) -> bool {
        self.window.is_wake_pending()
    }

    /// [`HarnessSession`](crate::HarnessSession)'s shot of `app`'s `region`.
    pub(super) fn shot(
        &mut self,
        app: &mut impl App,
        region: Option<Rect>,
    ) -> Result<Screenshot, PlayError> {
        if !self.paint.paints() {
            return Err(PlayError::NoPixels);
        }
        let asked = self
            .window
            .driver
            .frames
            .ui
            .request_screenshot(region)
            .ok_or(PlayError::EmptyRegion)?;
        // The frame paints the image; once the device's copy is back, the next frame lands it,
        // after any image an earlier ask is owed, one a frame.
        self.next_frame(app);
        for _ in 0..self.settle_frames {
            // A wait that timed out leaves the copy to a later frame.
            if self.wait_for_gpu() == GpuWait::DeviceLost {
                return Err(PlayError::DeviceLost);
            }
            self.next_frame(app);
            if let Some(shot) = self.window.driver.frames.ui.screenshot_taken(asked) {
                return Ok(shot.clone());
            }
        }
        // The last frame's submit can find the loss.
        if self.is_device_lost() {
            return Err(PlayError::DeviceLost);
        }
        Err(PlayError::Unsettled {
            frames: self.settle_frames,
        })
    }

    /// Render one frame of `app` at the clock as it stands, where [`Self::at`] parks it, and write
    /// the window's pixels into `pixels` in the target's format
    /// ([`Depth::Gpu`](crate::Depth::Gpu)'s `format`): rows top down with no
    /// padding, 4 bytes a pixel of 8-bit sRGB RGBA by default, 8 of `Rgba16Float`. That frame
    /// alone: no held input is delivered, the clock does not move, and no frame waits for the
    /// picture to land, as [a session's shot](crate::AgentWindow::shot) does. For a recording, which sets each frame's time.
    ///
    /// # Errors
    ///
    /// [`PlayError::NoPixels`] below [`Depth::Gpu`](crate::Depth::Gpu),
    /// [`PlayError::DeviceLost`] when the device is lost before the pixels come back, and
    /// [`PlayError::GpuTimedOut`] when it does not finish the frame in time; `pixels` is left
    /// empty on either.
    pub fn capture(&mut self, app: &mut impl App, pixels: &mut Vec<u8>) -> Result<(), PlayError> {
        if !self.paint.paints() {
            return Err(PlayError::NoPixels);
        }
        self.drive(app);
        let target = self
            .paint
            .target_mut()
            .expect("a harness that paints holds a target");
        match target.copy_texels(pixels) {
            GpuWait::Done => Ok(()),
            GpuWait::DeviceLost => Err(PlayError::DeviceLost),
            GpuWait::TimedOut => Err(PlayError::GpuTimedOut),
        }
    }

    /// Frames of `app` until it settles, as [a session's settle](crate::AgentWindow::settle) runs them. Only a frame that asks for another counts against
    /// [`settle_frames`](crate::HarnessBuilder::settle_frames): a frame a busy app's wake brought
    /// is its progress, and the wait for the next wake starts again after each.
    pub(super) fn settle_frames_of(&mut self, app: &mut impl App) -> Result<u32, PlayError> {
        let start = self.frames;
        let began = Instant::now();
        let busy_limit = self.background_wait * BUSY_SETTLE_WAITS;
        let mut asking = 0;
        loop {
            let report = self.next_frame(app);
            let busy = app.is_busy();
            let screenshots = self.window.driver.has_pending_screenshots();
            // A wake that is no busy work's progress asks for frames as an animation does.
            if report.repaint_requested || screenshots || (self.window.is_wake_pending() && !busy) {
                // A lost device lands no screenshot and paints no frame an animation asks for.
                if self.is_device_lost() {
                    return Err(PlayError::DeviceLost);
                }
                asking += 1;
                if asking == self.settle_frames {
                    return Err(PlayError::Unsettled {
                        frames: self.settle_frames,
                    });
                }
                // A wait that timed out leaves the copy to a later frame.
                if screenshots && self.wait_for_gpu() == GpuWait::DeviceLost {
                    return Err(PlayError::DeviceLost);
                }
                continue;
            }
            if busy {
                let waited = began.elapsed();
                if waited > busy_limit {
                    return Err(PlayError::StillBusy { waited });
                }
            }
            if self.window.is_wake_pending() {
                continue;
            }
            if !busy {
                return Ok(self.frames - start);
            }
            if !self
                .window
                .wait_for_wake(Instant::now() + self.background_wait)
            {
                return Err(PlayError::Busy {
                    waited: self.background_wait,
                });
            }
        }
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
    fn point(&mut self, at: &Locator<X>, reach: Reach) -> Result<Vec2, PlayError> {
        Ok(at.point_for(self.app, &self.h.window.driver.frames.ui, reach)?)
    }

    /// A modifier change that holds what is held already sends nothing, which would wait a frame.
    fn input(&mut self, event: InputEvent<'_>) {
        match event {
            InputEvent::ModifiersChanged(mods) => self.h.hold_modifiers(mods),
            event => self.h.on_input(event),
        };
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
        let interval = self.h.window.driver.frames.ui.double_click_interval();
        let since = self.h.time.saturating_sub(pressed);
        if since <= interval {
            self.h.advance(interval - since + Duration::from_millis(1));
        }
        Ok(())
    }

    fn note_press(&mut self) -> Result<(), PlayError> {
        self.h.last_press = Some(self.h.time);
        Ok(())
    }
}
