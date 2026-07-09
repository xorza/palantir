use crate::forest::shapes::record::ShapeStroke;
use crate::primitives::color::{Color, ColorF16};
use crate::primitives::corners::Corners;
use crate::primitives::rect::Rect;
use crate::primitives::stroke::Stroke;
use crate::renderer::frontend::cmd_buffer::{
    BrushSource, CmdKind, DrawRectPayload, DrawTrianglePayload, RenderCmdBuffer,
};
use glam::Vec2;

/// Both draw paths run the same stroke normalization inside the cmd
/// buffer (the single canonical correctness gate): a noop stroke —
/// transparent colour or zero width — lands in the payload as
/// `(TRANSPARENT, 0.0)`; anything else passes through verbatim. NaN
/// width is NOT a `ShapeStroke` noop (`noop_f16_bits` deliberately
/// classifies NaN as non-zero — loud bug, not silent skip), so it
/// passes through on both paths identically and the shader's
/// `stroke_width > 0.0` gate drops it. Table sweeps all four cases,
/// asserting the triangle payload's stroke fields are bit-identical to
/// the rect payload's for the same input stroke.
#[test]
fn triangle_stroke_normalization_matches_draw_rect() {
    let fill = Color::rgb(1.0, 0.0, 0.0);
    let green = Color::rgb(0.0, 1.0, 0.0);
    let cases: [(&str, ShapeStroke, bool); 4] = [
        (
            "transparent_color",
            Stroke::solid(Color::TRANSPARENT, 3.0).into(),
            true,
        ),
        ("zero_width", Stroke::solid(green, 0.0).into(), true),
        ("nan_width", Stroke::solid(green, f32::NAN).into(), false),
        ("live", Stroke::solid(green, 3.0).into(), false),
    ];
    for (label, stroke, expect_normalized) in cases {
        let mut rb = RenderCmdBuffer::default();
        rb.draw_rect(
            Rect::new(0.0, 0.0, 10.0, 10.0),
            Corners::ZERO,
            BrushSource::Solid(fill.into()),
            stroke,
        );
        assert_eq!(rb.kinds, vec![CmdKind::DrawRect], "case {label}");
        let rp: DrawRectPayload = rb.read(rb.starts[0]);

        let mut tb = RenderCmdBuffer::default();
        tb.draw_triangle(
            Vec2::ZERO,
            [
                Vec2::new(0.0, 0.0),
                Vec2::new(10.0, 0.0),
                Vec2::new(5.0, 8.0),
            ],
            fill.into(),
            0.0,
            stroke,
        );
        assert_eq!(tb.kinds, vec![CmdKind::DrawTriangle], "case {label}");
        let tp: DrawTrianglePayload = tb.read(tb.starts[0]);

        assert_eq!(tp.stroke_color, rp.stroke_color, "case {label}");
        assert_eq!(
            tp.stroke_width.to_bits(),
            rp.stroke_width.to_bits(),
            "case {label}",
        );
        if expect_normalized {
            assert_eq!(tp.stroke_color, ColorF16::TRANSPARENT, "case {label}");
            assert_eq!(tp.stroke_width, 0.0, "case {label}");
        } else {
            assert_eq!(tp.stroke_color, ColorF16::from(green), "case {label}");
        }
    }
}
