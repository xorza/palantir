//! A paint animation from the record call to the encoded draw.

use crate::host::harness::Harness;
use crate::internals::paint_capture::PaintCall;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::line_style::LineCap;
use crate::primitives::paint::stroke::Stroke;
use crate::shape::Shape;
use crate::shape::geometry::closed_geometry::PlacedClosed;
use crate::shape::paint_animation::PaintAnimation;
use crate::text::glyph_font::GlyphFont;
use crate::ui::Ui;
use crate::ui::tests::support::SURFACE;
use crate::widget_core::configure::Configure;
use crate::widgets::block::Block;
use glam::Vec2;
use std::time::Duration;

/// `add_shape` hands back the shape where the pair paints and `None` where it paints nothing, so
/// no animation reaches a shape that was never recorded: a white rect and a text run paint; the
/// same rect in a transparent fill, a rect of no width and empty text do not.
#[test]
fn add_shape_returns_none_where_the_pair_paints_nothing() {
    let mut h = Harness::new(SURFACE);
    let _ = h.frame(|ui| {
        let rect = Shape::rect().at(Rect::new(0.0, 0.0, 8.0, 8.0));
        let thin = Shape::rect().at(Rect::new(0.0, 0.0, 0.0, 8.0));
        let font = GlyphFont::new(12.0);
        let (label, empty) = (ui.intern("label"), ui.intern(""));
        assert!(ui.add_shape(rect, RgbaF32::WHITE).is_some(), "a white rect");
        assert!(
            ui.add_shape(Shape::text(label, font), RgbaF32::WHITE)
                .is_some(),
            "a text run"
        );
        assert!(
            ui.add_shape(rect, RgbaF32::TRANSPARENT).is_none(),
            "a transparent fill"
        );
        assert!(
            ui.add_shape(thin, RgbaF32::WHITE).is_none(),
            "a rect of no width"
        );
        assert!(
            ui.add_shape(Shape::text(empty, font), RgbaF32::WHITE)
                .is_none(),
            "empty text"
        );
    });
}

/// A fractional alpha survives the whole path (encoder sample, sink gate, payload color lane): opaque red with alpha lerping `0.0` → `1.0` over one second on a linear curve encodes a fill alpha of `0.5` at 500 ms. The crate's other animations answered only `0` or `1`.
#[test]
fn a_fractional_alpha_reaches_the_encoded_fill() {
    let record = |ui: &mut Ui| {
        Block::new()
            .id(WidgetId::from_hash("faded"))
            .size(20.0)
            .show(ui);
        if let Some(shape) = ui.add_shape(
            Shape::rect().at(Rect::new(0.0, 0.0, 8.0, 8.0)),
            RgbaF32::srgb(1.0, 0.0, 0.0),
        ) {
            shape.animate(PaintAnimation::alpha(0.0, 1.0).with_period(Duration::from_secs(1)));
        }
    };

    let mut h = Harness::mono(SURFACE);
    let _ = h.at(Duration::from_millis(500)).frame(record);
    let cmds = h.encode_paint();

    let alphas: Vec<f32> = cmds
        .calls
        .iter()
        .filter_map(|call| match call {
            PaintCall::Fill(p) => Some(RgbaF32::from(p.paint.fill.color).a),
            PaintCall::Shadow(p) => Some(RgbaF32::from(p.shadow.color).a),
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
    let mut h = Harness::new(SURFACE);
    let _ = h.frame(|ui| {
        Block::new()
            .id(WidgetId::from_hash("spun"))
            .size(20.0)
            .show(ui);
        if let Some(shape) = ui.add_shape(Shape::line(Vec2::ZERO, Vec2::X * 8.0), stroke) {
            shape.animate(quarter);
        }
        if let Some(shape) = ui.add_shape(
            Shape::line(Vec2::ONE, Vec2::ONE),
            stroke.with_cap(LineCap::Round),
        ) {
            shape.animate(quarter);
        }
        if let Some(shape) = ui.add_shape(
            Shape::rect().at(Rect::new(0.0, 0.0, 8.0, 8.0)),
            RgbaF32::WHITE,
        ) {
            shape.animate(quarter);
        }
        let label = ui.intern("turned");
        if let Some(shape) = ui.add_shape(Shape::text(label, GlyphFont::new(14.0)), RgbaF32::WHITE)
        {
            shape.animate(quarter);
        }
    });
    let cmds = h.encode_paint();
    let turns: Vec<bool> = cmds
        .calls
        .iter()
        .filter_map(|call| match call {
            PaintCall::Curve(p) => Some(p.bounds.spin().is_some()),
            PaintCall::Fill(p) => {
                Some(matches!(p.geometry, PlacedClosed::Box { .. }) && p.spin.is_some())
            }
            PaintCall::Text(p) => Some(p.spin.is_some()),
            _ => None,
        })
        .collect();
    assert_eq!(turns, [true, true, true, true]);
}

/// A quarter turn about a 20×20 owner's center (10, 10) reaches the encoded draws, each with its
/// pivot relative to the owner. A triangle, placed like a rect in the logical frame, keeps its
/// points and carries the pivot (10, 10); a mesh placed at (4, 4) carries it too, its place in the
/// fit that maps its vertices into the owner.
#[test]
fn a_turn_reaches_a_triangle_and_a_mesh() {
    use crate::primitives::geometry::fit::FitMap;
    use crate::primitives::geometry::mesh::Mesh;
    use crate::renderer::frontend::payload::spin::Spin;
    use crate::widgets::panel::Panel;
    use std::f32::consts::FRAC_PI_2;

    let mut mesh = Mesh::with_capacity(3, 3);
    let vertices =
        [Vec2::ZERO, Vec2::X * 8.0, Vec2::Y * 8.0].map(|pos| mesh.vertex(pos, RgbaF32::WHITE));
    mesh.triangle(vertices[0], vertices[1], vertices[2]);
    let quarter = PaintAnimation::turn(0.25, 0.25);

    let mut h = Harness::mono(SURFACE);
    let _ = h.frame(|ui| {
        Panel::zstack()
            .id(WidgetId::from_hash("spun"))
            .size(20.0)
            .show(ui, |ui| {
                if let Some(shape) = ui.add_shape(
                    Shape::triangle(Vec2::ZERO, Vec2::new(10.0, 0.0), Vec2::new(0.0, 10.0)),
                    RgbaF32::WHITE,
                ) {
                    shape.animate(quarter);
                }
                if let Some(shape) = ui.add_shape(
                    Shape::mesh(&mesh).at(Rect::new(4.0, 4.0, 8.0, 8.0)),
                    RgbaF32::WHITE,
                ) {
                    shape.animate(quarter);
                }
            });
    });
    let cmds = h.encode_paint();

    let triangle = cmds
        .calls
        .iter()
        .find_map(|call| match call {
            PaintCall::Fill(p) => match p.geometry {
                PlacedClosed::Triangle(placed) => Some((placed.centers, p.spin)),
                PlacedClosed::Box { .. } => None,
            },
            _ => None,
        })
        .expect("the triangle encodes");
    assert_eq!(
        triangle,
        (
            [Vec2::ZERO, Vec2::new(10.0, 0.0), Vec2::new(0.0, 10.0)],
            Some(Spin {
                pivot: Vec2::new(10.0, 10.0),
                angle: FRAC_PI_2,
            })
        )
    );

    let spin = cmds
        .calls
        .iter()
        .find_map(|call| match call {
            PaintCall::Mesh(p) => Some((p.spin, p.fit)),
            _ => None,
        })
        .expect("the mesh encodes");
    assert_eq!(
        spin,
        (
            Some(Spin {
                pivot: Vec2::new(10.0, 10.0),
                angle: FRAC_PI_2,
            }),
            FitMap {
                offset: Vec2::new(4.0, 4.0),
                scale: Vec2::ONE,
            },
        )
    );
}
