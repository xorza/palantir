//! The `Ui` a test drives, and the frames it records into.

use crate::Ui;
use crate::primitives::background::Background;
use crate::primitives::widget_id::WidgetId;
use crate::primitives::{color::RgbaF32, rect::Rect};
use crate::scene::node::configure::Configure;
use crate::scene::tree::node_id::NodeId;
use crate::ui::harness::UiHarness;
use crate::ui::resources::UiResources;
use crate::widgets::frame::Frame;
use glam::UVec2;
use std::time::Duration;

pub(super) const SURFACE: UVec2 = UVec2::new(200, 200);

pub(super) fn measure_calls(ui: &Ui) -> u64 {
    ui.resources.text().measure_calls()
}

pub(super) fn ui_with_shared(shared: &UiResources) -> UiHarness {
    UiHarness::from_resources(shared.clone(), SURFACE)
}

pub(super) fn blue_frame(ui: &mut Ui, salt: &'static str) -> NodeId {
    Frame::new()
        .id(WidgetId::from_hash(salt))
        .size(50.0)
        .background(Background {
            fill: RgbaF32::srgb(0.2, 0.4, 0.8).into(),
            ..Default::default()
        })
        .show(ui)
        .node()
}

pub(super) fn add_blink_shape(ui: &mut Ui, half: Duration) {
    use crate::scene::tree::paint_anims::PaintAnim;
    use crate::shape::Shape;

    ui.add_shape_animated(
        Shape::rect(Rect::new(0.0, 0.0, 4.0, 12.0)).fill(RgbaF32::srgb(1.0, 0.0, 0.0)),
        PaintAnim::BlinkOpacity {
            half_period: half,
            started_at: Duration::ZERO,
            stop_after: Duration::MAX,
        },
    );
}

pub(super) const COLD: UVec2 = UVec2::new(200, 200);

pub(super) fn cold_ui() -> UiHarness {
    UiHarness::cold(COLD)
}

pub(super) fn cold_frame(h: &mut UiHarness, record: impl FnMut(&mut Ui)) {
    let _ = h.frame(record);
}
