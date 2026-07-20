use crate::app::test_support::RecordApp;
use crate::host::shared::HostShared;
use crate::text::TextShaper;
use crate::ui::Ui;
use crate::ui::damage::region::DamageRegion;
use crate::ui::frame::{FrameInput, FrameStamp};
use crate::{Display, FrameReport, WindowToken};
use std::time::Duration;

fn mark_warm(ui: &mut Ui) {
    // Prevent cold-start warmup from invoking a fixture's record closure twice.
    ui.frame_runtime.prev_stamp = Some(FrameStamp::new(ui.display, std::time::Duration::ZERO));
}

impl Ui {
    pub(crate) fn record_test_frame_without_baseline(
        &mut self,
        display: Display,
        time: Duration,
        record: impl FnMut(&mut Ui),
    ) -> FrameReport {
        let mut app = RecordApp::new(record);
        self.frame(
            FrameInput {
                stamp: FrameStamp::new(display, time),
                damage_baseline_valid: false,
            },
            WindowToken(0),
            &mut app,
        )
    }

    pub fn record_test_frame(
        &mut self,
        display: Display,
        time: Duration,
        record: impl FnMut(&mut Ui),
    ) -> FrameReport {
        let mut app = RecordApp::new(record);
        self.frame(
            FrameInput {
                stamp: FrameStamp::new(display, time),
                damage_baseline_valid: true,
            },
            WindowToken(0),
            &mut app,
        )
    }

    pub(crate) fn for_test() -> Self {
        let mut ui = Self::default();
        mark_warm(&mut ui);
        ui
    }

    pub fn for_test_text() -> Self {
        thread_local! {
            static SHARED: TextShaper = TextShaper::with_bundled_fonts();
        }
        let shared = HostShared::new(SHARED.with(Clone::clone));
        let mut ui = Self::new(shared.ui_shared());
        mark_warm(&mut ui);
        ui
    }

    pub(crate) fn damage_region(&self) -> DamageRegion {
        DamageRegion::collapse_from(
            &self.damage_engine.raw_rects,
            self.damage_engine.budget_px,
            self.display.logical_rect(),
        )
    }
}
