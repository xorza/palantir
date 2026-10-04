//! The `Ui` a test drives, and the frames it records into.

use crate::Ui;
use crate::internals::harness::UiHarness;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::paint::background::Background;
use crate::primitives::paint::color::RgbaF32;
use crate::scene::tree::node_id::NodeId;
use crate::ui::resources::UiResources;
use crate::widget_core::configure::Configure;
use crate::widgets::block::Block;
use glam::UVec2;
use std::time::Duration;

pub(super) const SURFACE: UVec2 = UVec2::new(200, 200);

pub(super) fn ui_with_shared(shared: &UiResources) -> UiHarness {
    UiHarness::from_resources(shared.clone(), SURFACE)
}

pub(super) fn blue_frame(ui: &mut Ui, salt: &'static str) -> NodeId {
    Block::new()
        .id(WidgetId::from_hash(salt))
        .size(50.0)
        .background(Background::fill(RgbaF32::srgb(0.2, 0.4, 0.8)))
        .show(ui)
        .node()
}

pub(super) fn add_blink_shape(ui: &mut Ui, half: Duration) {
    use crate::scene::tree::paint_anims::curves;
    use crate::scene::tree::paint_anims::paint_animation::PaintAnimation;
    use crate::scene::tree::paint_anims::paint_animation::PaintRepeat;
    use crate::shape::Shape;

    ui.add_shape_animated(
        Shape::rect(Rect::new(0.0, 0.0, 4.0, 12.0)).fill(RgbaF32::srgb(1.0, 0.0, 0.0)),
        PaintAnimation::alpha(0.0, 1.0)
            .with_started_at(Duration::ZERO)
            .with_period(half * 2)
            .with_steps(2)
            .with_repeat(PaintRepeat::Settle(Duration::MAX))
            .with_curve(curves::square),
    );
}

/// A harness whose first frame runs the warmup pass, on [`SURFACE`].
pub(super) fn cold_ui() -> UiHarness {
    UiHarness::cold(SURFACE)
}
