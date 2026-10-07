//! A paint animation from the record call to the encoded draw.

use crate::Ui;
use crate::internals::harness::UiHarness;
use crate::internals::paint_capture::PaintCall;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::paint::color::RgbaF32;
use crate::scene::tree::paint_anims::curves;
use crate::scene::tree::paint_anims::paint_animation::PaintAnimation;
use crate::shape::Shape;
use crate::ui::tests::support::SURFACE;
use crate::widget_core::configure::Configure;
use crate::widgets::block::Block;
use std::time::Duration;

/// A fractional alpha survives the whole path (encoder sample, sink gate, payload colour lane): opaque red with alpha lerping `0.0` → `1.0` over one second on [`curves::linear`] encodes a fill alpha of `0.5` at 500 ms. The crate's other animations answered only `0` or `1`.
#[test]
fn a_fractional_alpha_reaches_the_encoded_fill() {
    let record = |ui: &mut Ui| {
        Block::new()
            .id(WidgetId::from_hash("faded"))
            .size(20.0)
            .show(ui);
        ui.add_shape_animated(
            Shape::rect(Rect::new(0.0, 0.0, 8.0, 8.0)).fill(RgbaF32::srgb(1.0, 0.0, 0.0)),
            PaintAnimation::alpha(0.0, 1.0)
                .with_period(Duration::from_secs(1))
                .with_curve(curves::linear),
        );
    };

    let mut h = UiHarness::new(SURFACE);
    let _ = h.at(Duration::from_millis(500)).frame(record);
    let cmds = h.encode_paint();

    let alphas: Vec<f32> = cmds
        .calls
        .iter()
        .filter_map(|call| match call {
            PaintCall::Quad(p) => Some(p.fill.color.unpack().a),
            _ => None,
        })
        .collect();
    assert_eq!(alphas, [0.5], "the one quad, encoded at half alpha");
}
