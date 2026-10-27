//! A paint animation from the record call to the encoded draw.

use crate::internals::paint_capture::PaintCall;
use crate::internals::panic_probe;
use crate::internals::ui_harness::UiHarness;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::stroke::Stroke;
use crate::shape::Shape;
use crate::shape::curve::CurveShape;
use crate::shape::paint_animation::PaintAnimation;
use crate::shape::paint_animation::curves;
use crate::shape::style::LineCap;
use crate::ui::Ui;
use crate::ui::tests::support::SURFACE;
use crate::widget_core::configure::Configure;
use crate::widgets::block::Block;
use glam::Vec2;
use std::time::Duration;

/// A fractional alpha survives the whole path (encoder sample, sink gate, payload color lane): opaque red with alpha lerping `0.0` → `1.0` over one second on [`curves::linear`] encodes a fill alpha of `0.5` at 500 ms. The crate's other animations answered only `0` or `1`.
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
            PaintCall::Quad(p) => Some(RgbaF32::from(p.fill.color).a),
            _ => None,
        })
        .collect();
    assert_eq!(alphas, [0.5], "the one quad, encoded at half alpha");
}

/// A turn rotates polylines and curves alone; on any other kind it panics rather than paint
/// unrotated, and a curve of no length, which paints its cap as a rect, is another kind.
#[test]
fn a_turn_on_a_shape_that_cannot_rotate_is_refused() {
    let turn = PaintAnimation::turn(0.0, 1.0);
    let stroke = Stroke::new(RgbaF32::WHITE, 2.0);
    let on = |shape: fn(Stroke) -> CurveShape| {
        move |ui: &mut Ui| {
            Block::new()
                .id(WidgetId::from_hash("spun"))
                .size(20.0)
                .show(ui);
            ui.add_shape_animated(shape(stroke), turn);
        }
    };
    let mut h = UiHarness::new(SURFACE);
    let _ = h.frame(on(|stroke| Shape::line(Vec2::ZERO, Vec2::X * 8.0, stroke)));

    let rule = "a turn paint animation rotates polylines and curves only";
    panic_probe::assert_panics_with(rule, || {
        UiHarness::new(SURFACE).frame(|ui| {
            ui.add_shape_animated(
                Shape::rect(Rect::new(0.0, 0.0, 8.0, 8.0)).fill(RgbaF32::WHITE),
                turn,
            );
        })
    });
    panic_probe::assert_panics_with(rule, || {
        UiHarness::new(SURFACE).frame(on(|stroke| {
            Shape::line(Vec2::ONE, Vec2::ONE, stroke).cap(LineCap::Round)
        }))
    });
}
