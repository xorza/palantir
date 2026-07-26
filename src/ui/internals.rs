//! The two raw frame entry points [`UiHarness`] is built on, plus the
//! tree/encoder reach-ins its in-crate rung forwards to.
//!
//! Nothing here is a test API in its own right — every caller goes
//! through [`UiHarness`], which owns the surface, the clock, and the
//! protocol rules. These stay on `Ui` only because they need
//! `Ui::frame`'s private `FrameInput`.
//!
//! [`UiHarness`]: crate::ui::harness::UiHarness

use crate::app::internals::RecordApp;
use crate::ui::Ui;
use crate::ui::frame::{FrameInput, FrameStamp};
use crate::{Display, FrameReport, WindowToken};
use std::time::Duration;

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

    pub(crate) fn record_test_frame(
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

    pub(crate) fn damage_region(&self) -> crate::scene::damage::region::DamageRegion {
        crate::scene::damage::region::DamageRegion::collapse_from(
            &self.damage_engine.raw_rects,
            self.damage_engine.budget_px,
            self.display.logical_rect(),
        )
    }
}

#[cfg(test)]
mod unit {
    use crate::Ui;
    use crate::animation::animatable::Animatable;
    use crate::layout::types::sizing::Sizing;
    use crate::primitives::rect::Rect;
    use crate::primitives::widget_id::WidgetId;
    use crate::renderer::frontend::encoder;
    use crate::renderer::frontend::record_sink::RecordedPaint;
    use crate::renderer::gradient_atlas::handle::SharedGradientAtlas;
    use crate::renderer::plan::{RenderKind, RenderPlan};
    use crate::scene::damage::region::DamageRegion;
    use crate::scene::layer::Layer;
    use crate::scene::node::Configure;
    use crate::scene::tree::node::NodeId;
    use crate::ui::harness::UiHarness;
    use crate::widgets::panel::Panel;

    impl Ui {
        pub(crate) fn node_for_widget_id(&self, id: WidgetId) -> NodeId {
            let tree = &self.forest.trees[Layer::Main];
            let idx = tree
                .records
                .widget_id()
                .iter()
                .position(|widget_id| *widget_id == id)
                .unwrap_or_else(|| panic!("no node found for widget_id {id:?}"));
            NodeId(idx as u32)
        }

        pub(crate) fn main_child_ids(&self, parent: NodeId) -> Vec<NodeId> {
            self.forest.trees[Layer::Main]
                .children(parent)
                .map(|child| child.id)
                .collect()
        }

        pub(crate) fn main_child_rects(&self, parent: NodeId) -> Vec<Rect> {
            self.forest.trees[Layer::Main]
                .children(parent)
                .map(|child| self.layout[Layer::Main].rect[child.id.idx()])
                .collect()
        }

        pub(crate) fn anim_row_count<T: Animatable>(&mut self) -> usize {
            self.anim
                .try_typed_mut::<T>()
                .map_or(0, |rows| rows.rows.len())
        }

        pub(crate) fn encode_paint(&self) -> RecordedPaint {
            let plan = RenderPlan {
                clear: self.theme.window_clear,
                kind: RenderKind::Full,
            };
            encoder::internals::encode(self.frame_scene(), &SharedGradientAtlas::default(), plan)
        }

        pub(crate) fn encode_paint_for(&self, region: DamageRegion) -> RecordedPaint {
            let plan = RenderPlan {
                clear: self.theme.window_clear,
                kind: RenderKind::Partial { region },
            };
            encoder::internals::encode(self.frame_scene(), &SharedGradientAtlas::default(), plan)
        }
    }

    impl UiHarness {
        /// A `FILL`/`FILL` hstack wrapped around `f`, returning the node
        /// `f` produced — the standard fixture for "arrange this one
        /// subtree against the whole surface".
        pub(crate) fn under_outer<F: FnMut(&mut Ui) -> NodeId>(&mut self, mut f: F) -> NodeId {
            self.frame_value_without_baseline(|ui| {
                Panel::hstack()
                    .auto_id()
                    .size((Sizing::FILL, Sizing::FILL))
                    .show(ui, &mut f)
                    .inner
            })
        }
    }
}
