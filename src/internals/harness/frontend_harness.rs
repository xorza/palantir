//! A [`UiHarness`] with a deviceless `Frontend` behind it.

use crate::internals::harness::UiHarness;
use crate::renderer::frontend::Frontend;
use crate::renderer::render_plan::RenderPlan;
use crate::scene::damage::Damage;
use crate::ui::Ui;
use crate::ui::frame_report::FrameReport;

/// Drives frames through the whole CPU pipeline: record through damage
/// on the [`UiHarness`], then encode and compose of what the frame
/// planned. It stops where a device would start, so nothing it does
/// waits on, or allocates for, a driver.
#[derive(Debug)]
pub struct FrontendHarness {
    pub(crate) harness: UiHarness,
    pub(crate) frontend: Frontend,
}

impl FrontendHarness {
    /// Paint `harness`'s frames into a fresh frontend with the baseline
    /// texture cap real adapters meet.
    pub fn new(harness: UiHarness) -> Self {
        Self {
            harness,
            frontend: Frontend::for_test(),
        }
    }

    /// The harness the frames run on, for input, the clock, the surface
    /// and the theme.
    pub const fn harness(&mut self) -> &mut UiHarness {
        &mut self.harness
    }

    /// One frame, encoded and composed when it planned a paint. A frame
    /// whose damage is empty skips both, as a host's would.
    pub fn frame(&mut self, record: impl FnMut(&mut Ui)) -> FrameReport {
        let report = self.harness.frame(record);
        if let Some(plan) = report.plan {
            self.frontend.build(self.harness.ui.frame_scene(), plan);
        }
        report
    }

    /// Encode and compose the whole retained scene, as a full repaint
    /// would, whatever the last frame planned.
    pub fn paint_full(&mut self) {
        let plan = RenderPlan {
            clear: self.harness.ui.theme().window_clear,
            damage: Damage::Full,
        };
        self.frontend.build(self.harness.ui.frame_scene(), plan);
    }
}
