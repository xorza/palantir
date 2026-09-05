//! A paint animation from the record call to the encoded draw.

use crate::primitives::color::RgbaF32;
use crate::primitives::rect::Rect;
use crate::primitives::widget_id::WidgetId;
use crate::renderer::frontend::capture::PaintCall;
use crate::scene::tree::paint_anims::curves;
use crate::scene::tree::paint_anims::paint_anim::PaintAnim;
use crate::shape::Shape;
use crate::ui::harness::UiHarness;
use crate::ui::tests::support::SURFACE;
use crate::widgets::configure::Configure;
use crate::widgets::frame::Frame;
use std::time::Duration;

/// A fractional alpha survives the whole path: sampled by the encoder,
/// folded by the sink's gate, and out in the payload's colour lane.
///
/// Hand-computed. The shape is opaque red and the animation lerps alpha
/// `0.0` → `1.0` over one second on [`curves::linear`], so the frame at
/// 500 ms encodes a fill alpha of `0.5`. Nothing before this could
/// produce one — the two animations the crate shipped answered `0` or
/// `1`, which is why the renderer half that carries a fraction had no
/// test of its own.
#[test]
fn a_fractional_alpha_reaches_the_encoded_fill() {
    let record = |ui: &mut crate::Ui| {
        Frame::new()
            .id(WidgetId::from_hash("faded"))
            .size(20.0)
            .show(ui);
        ui.add_shape_animated(
            Shape::rect(Rect::new(0.0, 0.0, 8.0, 8.0)).fill(RgbaF32::srgb(1.0, 0.0, 0.0)),
            PaintAnim::alpha(0.0, 1.0)
                .period(Duration::from_secs(1))
                .curve(curves::linear),
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
    assert!(
        alphas.iter().any(|a| (a - 0.5).abs() < 1e-2),
        "no quad encoded at half alpha: {alphas:?}",
    );
}
