//! A [`UiHarness`] with a deviceless `Frontend` behind it.

use crate::damage::Damage;
use crate::internals::ui_harness::UiHarness;
use crate::renderer::frontend::{BuildPlan, Frontend};
use crate::renderer::pane_captures::{PaneCaptures, PaneDamage, Settled};
use crate::renderer::render_plan::RenderPlan;
use crate::ui::Ui;
use crate::ui::frame_report::FrameReport;

/// Drives frames through the whole CPU pipeline, record through compose, stopping where a device would start.
#[derive(Debug)]
pub struct FrontendHarness {
    pub(crate) harness: UiHarness,
    pub(crate) frontend: Frontend,
    /// Which panes capture each frame, as a window driver settles them.
    pane_captures: PaneCaptures,
}

impl FrontendHarness {
    /// Paint `harness`'s frames into a fresh frontend.
    pub fn new(harness: UiHarness) -> Self {
        Self {
            harness,
            frontend: Frontend::for_test(),
            pane_captures: PaneCaptures::default(),
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
            self.build(plan);
        }
        report
    }

    /// Decide the frame's panes, encode and compose `plan`, and settle what the panes took; a
    /// partial frame whose captures overflow its scissors paints whole, as a window driver's does.
    fn build(&mut self, plan: RenderPlan) {
        let mut plan = plan;
        if self.decide(plan) == Settled::Overflow {
            plan = plan.to_full();
        }
        self.frontend.build(
            self.harness.ui.frame_scene(),
            BuildPlan {
                render: plan,
                panes: self.pane_captures.tickets(),
            },
        );
        self.pane_captures.settle(&self.frontend.buffer);
    }

    fn decide(&mut self, plan: RenderPlan) -> Settled {
        let scene = self.harness.ui.frame_scene();
        self.pane_captures.decide(
            scene.panes(),
            scene.stamp.display,
            PaneDamage::of(plan, self.harness.engines.damage.dirty_panes()),
        )
    }

    /// Encode and compose the whole retained scene, as a full repaint would.
    pub fn paint_full(&mut self) {
        let plan = RenderPlan {
            clear: self.harness.ui.theme().window_clear,
            damage: Damage::Full,
        };
        self.build(plan);
    }
}
