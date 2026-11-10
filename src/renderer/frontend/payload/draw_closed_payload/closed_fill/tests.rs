use crate::primitives::geometry::corners::Corners;
use crate::primitives::geometry::coverage::Coverage;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::packed::fill_axis::FillAxis;
use crate::primitives::packed::fill_kind::FillKind;
use crate::primitives::paint::border::Border;
use crate::primitives::paint::brush::gradient::Spread;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::color::rgba_f16::RgbaF16;
use crate::primitives::paint::lut_row::LutRow;
use crate::renderer::frontend::paint_sink::PaintPayload;
use crate::renderer::frontend::payload::brush_source::BrushSource;
use crate::renderer::frontend::payload::draw_closed_payload::DrawClosedPayload;
use crate::renderer::frontend::payload::draw_closed_payload::closed_fill::ClosedFill;
use crate::renderer::frontend::payload::resolved_gradient::ResolvedGradient;
use crate::shape::geometry::box_geometry::PlacedBox;
use crate::shape::geometry::closed_geometry::PlacedClosed;
use crate::shape::geometry::triangle_geometry::PlacedTriangle;
use crate::shape::paint::lowered_border::LoweredBorder;
use glam::Vec2;

/// The box (0, 0)..(10, 10), square-cornered, covering its inside.
fn square() -> PlacedClosed {
    PlacedClosed::inside(PlacedBox {
        rect: Rect::new(0.0, 0.0, 10.0, 10.0),
        corners: Corners::ZERO,
    })
}

/// A triangle of corners (0, 0), (10, 0), (5, 8), sharp.
fn triangle() -> PlacedClosed {
    PlacedClosed::Triangle(PlacedTriangle {
        centers: [Vec2::ZERO, Vec2::new(10.0, 0.0), Vec2::new(5.0, 8.0)],
        radius: 0.0,
    })
}

/// [`ClosedFill::new`], the one constructor of a fill whatever its shape, normalizes the border
/// ([`LoweredBorder::normalized`]): a transparent, zero-width or NaN-width border becomes
/// [`LoweredBorder::NONE`]; a live one passes verbatim.
///
/// NaN normalizes away like any non-painting width: catching it loudly is the `has_nan` screen's
/// job at `Shapes::push`, so here it fails safe.
#[test]
fn a_fill_normalizes_its_border() {
    let fill = BrushSource::Solid(RgbaF32::srgb(1.0, 0.0, 0.0).to_f16());
    let green = RgbaF32::srgb(0.0, 1.0, 0.0);
    let cases: [(&str, Border, LoweredBorder); 4] = [
        (
            "transparent_color",
            Border::new(RgbaF32::TRANSPARENT, 3.0),
            LoweredBorder::NONE,
        ),
        ("zero_width", Border::new(green, 0.0), LoweredBorder::NONE),
        (
            "nan_width",
            Border::new(green, f32::NAN),
            LoweredBorder::NONE,
        ),
        (
            "live",
            Border::new(green, 3.0),
            LoweredBorder {
                width: 3.0,
                color: green.to_f16(),
            },
        ),
    ];
    for (label, border, expected) in cases {
        let lowered = ClosedFill::new(fill, LoweredBorder::lower(border)).border;
        assert_eq!(lowered.color, expected.color, "case {label}");
        assert_eq!(
            lowered.width.to_bits(),
            expected.width.to_bits(),
            "case {label}"
        );
    }
}

/// A fill paints nothing over a box of no extent, whatever it is; over a triangle, whose covering
/// rect exists only after transform, it paints while its fill or its border shows.
#[test]
fn a_fill_paints_nothing_over_an_empty_box() {
    let white = ClosedFill::new(
        BrushSource::Solid(RgbaF32::WHITE.to_f16()),
        LoweredBorder::NONE,
    );
    let empty = PlacedClosed::inside(PlacedBox {
        rect: Rect::new(4.0, 4.0, 0.0, 10.0),
        corners: Corners::ZERO,
    });
    assert!(!DrawClosedPayload::new(square(), white).is_noop());
    assert!(DrawClosedPayload::new(empty, white).is_noop());
    assert!(!DrawClosedPayload::new(triangle(), white).is_noop());
    let clear = ClosedFill::new(
        BrushSource::Solid(RgbaF16::TRANSPARENT),
        LoweredBorder::NONE,
    );
    assert!(DrawClosedPayload::new(triangle(), clear).is_noop());
}

/// A fade reaches a solid fill and a gradient fill through the one alpha
/// lane. A solid scales its real alpha (half of `0.8` is `0.4`); a
/// gradient's lane multiplies the sample, starting white, so a half fade
/// gives alpha `0.5` with RGB at one. Fading to zero makes either a no-op,
/// the border scales either way, and `faded(1.0)` changes nothing.
#[test]
fn a_fade_reaches_the_solid_alpha_and_the_gradients_multiplier() {
    let border = LoweredBorder {
        width: 2.0,
        color: RgbaF16::new(1.0, 1.0, 1.0, 1.0),
    };
    let quad = |fill| DrawClosedPayload::new(square(), ClosedFill::new(fill, border));

    let solid = quad(BrushSource::Solid(RgbaF16::new(0.25, 0.5, 0.75, 0.8)));
    assert_eq!(solid.faded(1.0), solid);
    let faded = solid.faded(0.5);
    // 0.8 packs to f16 as 1638 steps of 2^-11, and halving it is exact.
    assert_eq!(RgbaF32::from(faded.paint.fill.color).a, 1638.0 / 4096.0);
    assert_eq!(RgbaF32::from(faded.paint.border.color).a, 0.5);

    let gradient = quad(BrushSource::Gradient(ResolvedGradient {
        axis: FillAxis::ZERO,
        lut_row: LutRow::FALLBACK,
        kind: FillKind::linear(Spread::Pad),
    }));
    assert_eq!(
        gradient.paint.fill.color,
        RgbaF16::WHITE,
        "an unfaded gradient multiplies its sample by one",
    );
    let faded = gradient.faded(0.5);
    let multiplier = RgbaF32::from(faded.paint.fill.color);
    assert_eq!((multiplier.r, multiplier.g, multiplier.b), (1.0, 1.0, 1.0));
    assert_eq!(multiplier.a, 0.5);
    assert_eq!(faded.paint.fill.lut_row, gradient.paint.fill.lut_row);

    assert!(!solid.is_noop() && !gradient.is_noop());
    assert!(solid.faded(0.0).is_noop());
    assert!(
        gradient.faded(0.0).is_noop(),
        "a gradient faded out paints nothing"
    );
}

/// Only a solid fill over the inside of a box can cover its rect whole. Each other fill differs
/// from that one draw in one respect: an outside rect's hole, a triangle's corners and a
/// gradient's stops each leave part of the rect uncovered or unknown.
#[test]
fn only_a_solid_inside_box_is_a_solid_box() {
    let solid = ClosedFill::new(
        BrushSource::Solid(RgbaF32::WHITE.to_f16()),
        LoweredBorder::NONE,
    );
    let PlacedClosed::Box { placed, .. } = square() else {
        unreachable!("a square is a box");
    };
    assert!(DrawClosedPayload::new(square(), solid).is_solid_box());
    let outside = PlacedClosed::Box {
        placed,
        coverage: Coverage::Outside,
    };
    assert!(!DrawClosedPayload::new(outside, solid).is_solid_box());
    let gradient = ClosedFill::new(
        BrushSource::Gradient(ResolvedGradient {
            axis: FillAxis::ZERO,
            lut_row: LutRow::FALLBACK,
            kind: FillKind::linear(Spread::Pad),
        }),
        LoweredBorder::NONE,
    );
    assert!(!DrawClosedPayload::new(square(), gradient).is_solid_box());
    assert!(!DrawClosedPayload::new(triangle(), solid).is_solid_box());
}
