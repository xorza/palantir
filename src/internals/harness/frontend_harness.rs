//! A [`UiHarness`] with a deviceless `Frontend` behind it.

use crate::damage::Damage;
use crate::internals::harness::UiHarness;
use crate::renderer::frontend::Frontend;
use crate::renderer::render_plan::RenderPlan;
use crate::ui::Ui;
use crate::ui::frame_report::FrameReport;

/// Drives frames through the whole CPU pipeline, record through compose, stopping where a device would start.
#[derive(Debug)]
pub struct FrontendHarness {
    pub(crate) harness: UiHarness,
    pub(crate) frontend: Frontend,
}

impl FrontendHarness {
    /// Paint `harness`'s frames into a fresh frontend.
    pub fn new(harness: UiHarness) -> Self {
        Self {
            harness,
            frontend: Frontend::for_test(),
        }
    }

    /// The harness the frames run on.
    pub const fn harness(&mut self) -> &mut UiHarness {
        &mut self.harness
    }

    /// One frame, encoded and composed when it planned a paint.
    pub fn frame(&mut self, record: impl FnMut(&mut Ui)) -> FrameReport {
        let report = self.harness.frame(record);
        if let Some(plan) = report.plan {
            self.frontend.build(self.harness.ui.frame_scene(), plan);
        }
        report
    }

    /// Encode and compose the whole retained scene, as a full repaint would.
    pub fn paint_full(&mut self) {
        let plan = RenderPlan {
            clear: self.harness.ui.theme().window_clear,
            damage: Damage::Full,
        };
        self.frontend.build(self.harness.ui.frame_scene(), plan);
    }
}
