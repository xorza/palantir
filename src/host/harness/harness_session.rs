//! [`HarnessSession`]: a [`Harness`] and its app, driven as an [`AgentWindow`].

use crate::driver::action::Action;
use crate::driver::agent_window::AgentWindow;
use crate::driver::error::{ActError, PlayError};
use crate::driver::locator::Locate;
use crate::host::harness::Harness;
use crate::primitives::geometry::rect::Rect;
use crate::ui::Ui;
use crate::ui::app::App;
use crate::window::screenshot::Screenshot;

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
    /// pointer first and settles, so it lands on what the pointer hovers. Returns the frames run.
    ///
    /// # Errors
    ///
    /// [`ActError`], naming the step it stopped at.
    fn play<X: Locate<A>>(&mut self, actions: &[Action<X>]) -> Result<u32, ActError> {
        self.harness.play(self.app, actions)
    }

    /// Run frames of the app, the clock moving one frame's time before each, until one asks no
    /// repaint, no screenshot is on its way back from the device, which it waits for, no
    /// [wake](crate::Waker) waits, and the app is not [busy](App::is_busy). While the app is busy
    /// and asks nothing, the settle waits in real time for each wake its work sends, as long as
    /// the work goes on. A caret blink asks a timed wake, not a repaint, so it settles. Returns
    /// the frames run.
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
        self.harness.shot(self.app, region)
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
}
