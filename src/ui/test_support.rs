use crate::app::test_support::RecordApp;
use crate::host::shared::HostShared;
use crate::scene::damage::region::DamageRegion;
use crate::text::TextShaper;
use crate::ui::Ui;
use crate::ui::frame::{FrameInput, FrameStamp};
use crate::{Display, FrameReport, WindowToken};
use std::time::Duration;

fn mark_warm(ui: &mut Ui) {
    // Prevent cold-start warmup from invoking a fixture's record closure twice.
    ui.frame_runtime.prev_stamp = Some(FrameStamp::new(ui.display, Duration::ZERO));
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
        let mut ui = Self::new(shared.resources.clone());
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

#[cfg(test)]
mod unit {
    use crate::FrameReport;
    use crate::Ui;
    use crate::animation::animatable::Animatable;
    use crate::display::Display;
    use crate::input::InputEvent;
    use crate::input::pointer::PointerButton;
    use crate::layout::scroll::ScrollLayoutState;
    use crate::primitives::rect::Rect;
    use crate::primitives::widget_id::WidgetId;
    use crate::renderer::frontend::cmd_buffer::RenderCmdBuffer;
    use crate::renderer::frontend::encoder;
    use crate::renderer::gradient_atlas::handle::SharedGradientAtlas;
    use crate::renderer::plan::{RenderKind, RenderPlan};
    use crate::scene::damage::region::DamageRegion;
    use crate::scene::element::Configure;
    use crate::scene::layer::Layer;
    use crate::scene::tree::node::NodeId;
    use crate::ui::frame::FrameStamp;
    use crate::widgets::panel::Panel;
    use glam::{UVec2, Vec2};
    use std::time::Duration;

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

        pub(crate) fn for_test_at(size: UVec2) -> Self {
            let display = Display::from_physical(size, 1.0);
            let mut ui = Self {
                display,
                ..Self::default()
            };
            ui.frame_runtime.prev_stamp = Some(FrameStamp::new(display, Duration::ZERO));
            ui
        }

        pub(crate) fn for_test_at_text(size: UVec2) -> Self {
            let display = Display::from_physical(size, 1.0);
            let mut ui = Self::for_test_text();
            ui.display = display;
            ui.frame_runtime.prev_stamp = Some(FrameStamp::new(display, Duration::ZERO));
            ui
        }

        pub(crate) fn run_at(&mut self, size: UVec2, record: impl FnMut(&mut Ui)) -> FrameReport {
            self.record_test_frame(Display::from_physical(size, 1.0), Duration::ZERO, record)
        }

        pub(crate) fn run_at_without_baseline(
            &mut self,
            size: UVec2,
            record: impl FnMut(&mut Ui),
        ) -> FrameReport {
            self.record_test_frame_without_baseline(
                Display::from_physical(size, 1.0),
                Duration::ZERO,
                record,
            )
        }

        pub(crate) fn run_at_value<R>(
            &mut self,
            size: UVec2,
            mut record: impl FnMut(&mut Ui) -> R,
        ) -> R {
            let mut value = None;
            self.run_at(size, |ui| value = Some(record(ui)));
            value.expect("test frame did not run a record pass")
        }

        pub(crate) fn run_at_value_without_baseline<R>(
            &mut self,
            size: UVec2,
            mut record: impl FnMut(&mut Ui) -> R,
        ) -> R {
            let mut value = None;
            self.run_at_without_baseline(size, |ui| value = Some(record(ui)));
            value.expect("test frame did not run a record pass")
        }

        pub(crate) fn under_outer<F: FnMut(&mut Ui) -> NodeId>(
            &mut self,
            surface: UVec2,
            mut f: F,
        ) -> NodeId {
            use crate::layout::types::sizing::Sizing;

            self.run_at_value_without_baseline(surface, |ui| {
                Panel::hstack()
                    .auto_id()
                    .size((Sizing::FILL, Sizing::FILL))
                    .show(ui, &mut f)
                    .inner
            })
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

        pub(crate) fn click_at(&mut self, pos: Vec2) {
            self.on_input(InputEvent::PointerMoved(pos));
            self.on_input(InputEvent::PointerPressed(PointerButton::Left));
            self.on_input(InputEvent::PointerReleased(PointerButton::Left));
        }

        pub(crate) fn press_at(&mut self, pos: Vec2) {
            self.on_input(InputEvent::PointerMoved(pos));
            self.on_input(InputEvent::PointerPressed(PointerButton::Left));
        }

        pub(crate) fn release_left(&mut self) {
            self.on_input(InputEvent::PointerReleased(PointerButton::Left));
        }

        pub(crate) fn secondary_click_at(&mut self, pos: Vec2) {
            self.on_input(InputEvent::PointerMoved(pos));
            self.on_input(InputEvent::PointerPressed(PointerButton::Right));
            self.on_input(InputEvent::PointerReleased(PointerButton::Right));
        }

        pub(crate) fn scroll_state(&mut self, id: WidgetId) -> &mut ScrollLayoutState {
            self.layout_engine.scroll_states.entry(id).or_default()
        }

        pub(crate) fn anim_row_count<T: Animatable>(&mut self) -> usize {
            self.anim
                .try_typed_mut::<T>()
                .map_or(0, |rows| rows.rows.len())
        }

        pub(crate) fn encode_cmds(&self) -> RenderCmdBuffer {
            let plan = RenderPlan {
                clear: self.theme.window_clear,
                kind: RenderKind::Full,
            };
            encoder::test_support::encode(self.frame_scene(), &SharedGradientAtlas::default(), plan)
        }

        pub(crate) fn encode_cmds_for(&self, region: DamageRegion) -> RenderCmdBuffer {
            let plan = RenderPlan {
                clear: self.theme.window_clear,
                kind: RenderKind::Partial { region },
            };
            encoder::test_support::encode(self.frame_scene(), &SharedGradientAtlas::default(), plan)
        }
    }
}
