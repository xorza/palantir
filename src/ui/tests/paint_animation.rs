//! A paint animation from the record call to the encoded draw.

use crate::internals::paint_capture::PaintCall;
use crate::internals::ui_harness::UiHarness;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::stroke::Stroke;
use crate::renderer::frontend::payload::draw_quad_payload::QuadGeometry;
use crate::shape::Shape;
use crate::shape::paint_animation::PaintAnimation;
use crate::shape::paint_animation::curves;
use crate::shape::style::LineCap;
use crate::text::glyph_font::GlyphFont;
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

/// A turn records on every kind, and reaches the draws the encoder makes: here a line, a dot (a
/// curve of no length, painted as a quad), a rect and a text run each carry the quarter turn.
#[test]
fn a_turn_records_on_every_kind() {
    let quarter = PaintAnimation::turn(0.25, 0.25);
    let stroke = Stroke::new(RgbaF32::WHITE, 2.0);
    let mut h = UiHarness::with_text(SURFACE);
    let _ = h.frame(|ui| {
        Block::new()
            .id(WidgetId::from_hash("spun"))
            .size(20.0)
            .show(ui);
        ui.add_shape_animated(Shape::line(Vec2::ZERO, Vec2::X * 8.0, stroke), quarter);
        ui.add_shape_animated(
            Shape::line(Vec2::ONE, Vec2::ONE, stroke).cap(LineCap::Round),
            quarter,
        );
        ui.add_shape_animated(
            Shape::rect(Rect::new(0.0, 0.0, 8.0, 8.0)).fill(RgbaF32::WHITE),
            quarter,
        );
        let label = ui.intern("turned");
        ui.add_shape_animated(Shape::text(label, GlyphFont::new(14.0)), quarter);
    });
    let cmds = h.encode_paint();
    let turns: Vec<bool> = cmds
        .calls
        .iter()
        .filter_map(|call| match call {
            PaintCall::Curve(p) => Some(p.bounds.spin().is_some()),
            PaintCall::Quad(p) => Some(matches!(
                p.geometry,
                QuadGeometry::Rect { spin: Some(_), .. }
            )),
            PaintCall::Text(p) => Some(p.spin.is_some()),
            _ => None,
        })
        .collect();
    assert_eq!(turns, [true, true, true, true]);
}

/// A quarter turn about a 20×20 owner's center (10, 10) reaches the encoded draws. A triangle's
/// owner-local points turn in the encoder: `(x, y) ↦ (10 − (y − 10), 10 + (x − 10))` takes
/// (0, 0), (10, 0), (0, 10) to (20, 0), (20, 10), (10, 0). A mesh placed at (4, 4) carries the turn
/// with its pivot in its own frame, (10, 10) − (4, 4) = (6, 6). The tolerance is the f32 cosine of
/// a quarter turn, −4.4e-8, not 0, times the lengths involved.
#[test]
fn a_turn_reaches_a_triangle_and_a_mesh() {
    use crate::primitives::geometry::mesh::Mesh;
    use crate::renderer::frontend::payload::spin::Spin;
    use crate::widgets::panel::Panel;
    use std::f32::consts::FRAC_PI_2;

    const EPS: f32 = 1e-5;
    let mut mesh = Mesh::with_capacity(3, 3);
    let vertices =
        [Vec2::ZERO, Vec2::X * 8.0, Vec2::Y * 8.0].map(|pos| mesh.vertex(pos, RgbaF32::WHITE));
    mesh.triangle(vertices[0], vertices[1], vertices[2]);
    let quarter = PaintAnimation::turn(0.25, 0.25);

    let mut h = UiHarness::new(SURFACE);
    let _ = h.frame(|ui| {
        Panel::zstack()
            .id(WidgetId::from_hash("spun"))
            .size(20.0)
            .show(ui, |ui| {
                ui.add_shape_animated(
                    Shape::triangle(Vec2::ZERO, Vec2::new(10.0, 0.0), Vec2::new(0.0, 10.0))
                        .fill(RgbaF32::WHITE),
                    quarter,
                );
                ui.add_shape_animated(
                    Shape::mesh(&mesh).at(Rect::new(4.0, 4.0, 8.0, 8.0)),
                    quarter,
                );
            });
    });
    let cmds = h.encode_paint();

    let triangle = cmds
        .calls
        .iter()
        .find_map(|call| match call {
            PaintCall::Quad(p) => match p.geometry {
                QuadGeometry::Triangle { a, b, c, .. } => Some([a, b, c]),
                QuadGeometry::Rect { .. } => None,
            },
            _ => None,
        })
        .expect("the triangle encodes");
    let want = [
        Vec2::new(20.0, 0.0),
        Vec2::new(20.0, 10.0),
        Vec2::new(10.0, 0.0),
    ];
    for (got, want) in triangle.into_iter().zip(want) {
        assert!(got.abs_diff_eq(want, EPS), "{got:?} vs {want:?}");
    }

    let spin = cmds
        .calls
        .iter()
        .find_map(|call| match call {
            PaintCall::Mesh(p) => Some(p.spin),
            _ => None,
        })
        .expect("the mesh encodes");
    assert_eq!(
        spin,
        Some(Spin {
            pivot: Vec2::new(6.0, 6.0),
            angle: FRAC_PI_2,
        })
    );
}
