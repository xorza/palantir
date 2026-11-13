//! What the display scale and a transform do to what is drawn.

use crate::common::span::Span;
use crate::primitives::geometry::corners::Corners;
use crate::primitives::geometry::fit::FitMap;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::geometry::size::Size;
use crate::primitives::geometry::translate_scale::TranslateScale;
use crate::primitives::geometry::urect::URect;
use crate::primitives::identity::icon_ref::IconRef;
use crate::primitives::identity::texture_id::TextureId;
use crate::primitives::packed::fill_axis::FillAxis;
use crate::primitives::packed::fill_kind::FillKind;
use crate::primitives::paint::border::Border;
use crate::primitives::paint::brush::gradient::Spread;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::color::rgba_f16::RgbaF16;
use crate::primitives::paint::line_style::{LineCap, LineJoin};
use crate::primitives::paint::lut_row::LutRow;
use crate::renderer::frontend::composer::stroke_bbox::StrokeBbox;
use crate::renderer::frontend::composer::tests::quad_builder::QuadBuilder;
use crate::renderer::frontend::composer::tests::support::{
    clip, draw, params, params_unsnapped, quad_runs, rows_of, run, text,
};
use crate::renderer::frontend::paint_sink::{PaintPayload, PaintSink};
use crate::renderer::frontend::payload::brush_source::BrushSource;
use crate::renderer::frontend::payload::draw_closed_payload::DrawClosedPayload;
use crate::renderer::frontend::payload::draw_closed_payload::closed_fill::ClosedFill;
use crate::renderer::frontend::payload::draw_closed_payload::closed_image::ClosedImage;
use crate::renderer::frontend::payload::draw_mesh_payload::DrawMeshPayload;
use crate::renderer::frontend::payload::draw_shadow_payload::DrawShadowPayload;
use crate::renderer::frontend::payload::resolved_gradient::ResolvedGradient;
use crate::renderer::frontend::payload::spin::Spin;
use crate::renderer::render_buffer::draw_run::Lane;
use crate::renderer::render_buffer::fill_form::{FillBrush, FillForm};
use crate::renderer::render_buffer::image_flags::ImageFlags;
use crate::renderer::render_buffer::paint_tier::PaintTier;
use crate::renderer::render_buffer::quad_draw_row::QuadKey;
use crate::renderer::render_buffer::shadow_instance::{ShadowInstance, ShadowKind};
use crate::renderer::render_buffer::shape_form::ShapeForm;
use crate::shape::geometry::box_geometry::PlacedBox;
use crate::shape::geometry::closed_geometry::PlacedClosed;
use crate::shape::geometry::triangle_geometry::PlacedTriangle;
use crate::shape::paint::lowered_border::LoweredBorder;
use crate::shape::paint::lowered_shadow::LoweredShadow;
use crate::shape::stroke_bounds::StrokeJoints;
use crate::text::TEXT_SCALE_STEP;
use glam::{UVec2, Vec2};
use std::f32::consts::FRAC_PI_2;

#[test]
fn stroke_bbox_urect_applies_transform_dpi_and_style_once() {
    #[derive(Debug)]
    struct Case {
        scale: f32,
        cap: LineCap,
        joints: StrokeJoints,
        expected: URect,
    }

    // Centerline (10,20)..(30,30), plus origin (2,4), then
    // x ↦ 1.5x + (3,5) gives logical (21,41)..(51,56).
    // Butt cases use physical pad = width_physical/2 + 0.5:
    // 0.5× → 2, 1× → 3.5, 2× → 6.5.
    let cases = [
        Case {
            scale: 0.5,
            cap: LineCap::Butt,
            joints: StrokeJoints::Unjoined,
            expected: URect::new(8, 18, 20, 12),
        },
        Case {
            scale: 1.0,
            cap: LineCap::Butt,
            joints: StrokeJoints::Unjoined,
            expected: URect::new(17, 37, 38, 23),
        },
        Case {
            scale: 2.0,
            cap: LineCap::Butt,
            joints: StrokeJoints::Unjoined,
            expected: URect::new(35, 75, 74, 44),
        },
        // At 1×, Square pad = 3.5√2 ≈ 4.9498.
        Case {
            scale: 1.0,
            cap: LineCap::Square,
            joints: StrokeJoints::Unjoined,
            expected: URect::new(16, 36, 40, 25),
        },
        // At 1×, Miter pad = 3.5·4 = 14.
        Case {
            scale: 1.0,
            cap: LineCap::Butt,
            joints: StrokeJoints::Joined(LineJoin::Miter),
            expected: URect::new(7, 27, 58, 43),
        },
    ];
    let transform = TranslateScale::new(Vec2::new(3.0, 5.0), 1.5);

    for case in cases {
        let actual = StrokeBbox {
            transform,
            bbox: Rect::new(10.0, 20.0, 20.0, 10.0),
            origin: Vec2::new(2.0, 4.0),
            width_physical: 4.0 * 1.5 * case.scale,
            cap: case.cap,
            joints: case.joints,
            display: params(case.scale, UVec2::new(200, 200)),
        }
        .urect();
        assert_eq!(actual, case.expected, "{case:?}");
    }
}

/// A NaN border width normalizes away like any non-painting width, uniformly
/// for every quad shape. `Shapes::push` catches it loudly at authoring; this
/// pins the release-side fallback end to end: no NaN reaches the GPU, on
/// either geometry.
#[test]
fn nan_border_width_normalizes_away_on_every_quad_geometry() {
    let nan_border = Border::new(RgbaF32::srgb(0.0, 1.0, 0.0), f32::NAN);
    let display = params(2.0, UVec2::new(400, 400));

    // An opaque fill keeps the draw alive so the border lanes can be inspected.
    let buf = run(
        |b, _arena| {
            QuadBuilder::new(Rect::new(10.0, 20.0, 30.0, 40.0))
                .border(nan_border)
                .draw(b);
        },
        &display,
    );
    assert_eq!(buf.quads.len(), 1, "the fill keeps the rect alive");
    assert_eq!(
        buf.quads.instance()[0].border_width,
        0.0,
        "a NaN width must not reach the instance",
    );

    let buf = run(
        |b, _arena| {
            b.draw(
                DrawClosedPayload::new(
                    PlacedClosed::Triangle(PlacedTriangle {
                        centers: [
                            Vec2::new(0.0, 0.0),
                            Vec2::new(10.0, 0.0),
                            Vec2::new(5.0, 8.0),
                        ],
                        radius: 0.0,
                    }),
                    ClosedFill::new(
                        BrushSource::Solid(RgbaF32::WHITE.to_f16()),
                        LoweredBorder::lower(nan_border),
                    ),
                ),
                1.0,
            );
        },
        &display,
    );
    assert_eq!(buf.quads.len(), 1, "the fill keeps the triangle alive");
    assert_eq!(
        buf.quads.instance()[0].border_width,
        0.0,
        "the triangle arm must agree with the rect arm",
    );

    // A NaN border on an otherwise unpainted shape is dropped.
    let buf = run(
        |b, _arena| {
            QuadBuilder::new(Rect::new(10.0, 20.0, 30.0, 40.0))
                .solid(RgbaF32::TRANSPARENT)
                .border(nan_border)
                .draw(b);
        },
        &display,
    );
    assert!(buf.quads.is_empty(), "nothing to paint, nothing emitted");
}

#[test]
fn compose_scales_rects_for_dpr() {
    let buf = run(
        |b, _arena| draw(b, Rect::new(10.0, 20.0, 30.0, 40.0)),
        &params(2.0, UVec2::new(400, 400)),
    );
    assert_eq!(buf.quads.len(), 1);
    let q = &buf.quads.instance()[0];
    assert_eq!(q.header.rect.min, Vec2::new(20.0, 40.0));
    assert_eq!(q.header.rect.size, Size::new(60.0, 80.0));
}

#[test]
fn compose_translates_under_push_transform() {
    let buf = run(
        |b, _arena| {
            b.push_transform(TranslateScale::from_translation(Vec2::new(100.0, 50.0)));
            draw(b, Rect::new(10.0, 20.0, 30.0, 40.0));
            b.pop_transform();
        },
        &params(1.0, UVec2::new(400, 400)),
    );
    assert_eq!(buf.quads.len(), 1);
    let q = &buf.quads.instance()[0];
    assert_eq!(q.header.rect.min, Vec2::new(110.0, 70.0));
    assert_eq!(q.header.rect.size, Size::new(30.0, 40.0));
}

#[test]
fn compose_scales_radius_and_border_under_transform() {
    let buf = run(
        |b, _arena| {
            b.push_transform(TranslateScale::from_scale(2.0));
            QuadBuilder::new(Rect::new(0.0, 0.0, 50.0, 50.0))
                .corners(Corners::all(8.0))
                .border(Border::new(RgbaF32::srgb(0.0, 0.0, 0.0), 1.5))
                .draw(b);
            b.pop_transform();
        },
        &params(1.0, UVec2::new(400, 400)),
    );
    let q = &buf.quads.instance()[0];
    assert_eq!(q.header.rect.size, Size::new(100.0, 100.0));
    assert_eq!(q.header.corners.to_array()[0], 16.0);
    assert_eq!(q.border_width, 3.0);
}

/// Text-run scale snaps to the additive 0.5% ladder so continuous zoom gives
/// stable glyph cache keys. Quads (next test) do not snap.
#[test]
fn compose_snaps_text_scale_to_discrete_steps() {
    // 1.013 is between 1.010 and 1.015; rounds to 1.015.
    let buf = run(
        |b, _arena| {
            b.push_transform(TranslateScale::from_scale(1.013));
            text(b, Rect::new(0.0, 0.0, 50.0, 20.0));
            b.pop_transform();
        },
        &params(1.0, UVec2::new(400, 400)),
    );
    assert_eq!(buf.texts.len(), 1);
    // 1.013 / 0.005 = 202.6, which rounds to rung 203: 1.015.
    assert_eq!(
        buf.texts[0].scale,
        203.0 * TEXT_SCALE_STEP,
        "1.013 must snap to 1.015",
    );
}

/// A quad under the same fractional transform keeps its continuous scale;
/// only text snaps.
#[test]
fn compose_keeps_quad_scale_continuous_under_zoom() {
    let buf = run(
        |b, _arena| {
            b.push_transform(TranslateScale::from_scale(1.013));
            draw(b, Rect::new(0.0, 0.0, 100.0, 50.0));
            b.pop_transform();
        },
        // Unsnapped: the pixel snap rounds every quad's edges; no scale rung applies on top.
        &params_unsnapped(1.0, UVec2::new(400, 400)),
    );
    assert_eq!(buf.quads.len(), 1);
    // 100 × 1.013 and 50 × 1.013 — preserved, not snapped.
    assert_eq!(
        buf.quads.instance()[0].header.rect.size,
        Size::new(100.0 * 1.013, 50.0 * 1.013)
    );
}

#[test]
fn compose_propagates_transform_scale_to_text_runs() {
    // A `TranslateScale(_, 2.0)` ancestor must surface on the emitted
    // `TextDrawRow.scale`, or glyph rasters stay at the shaped size.
    let buf = run(
        |b, _arena| {
            b.push_transform(TranslateScale::from_scale(2.0));
            text(b, Rect::new(0.0, 0.0, 50.0, 20.0));
            b.pop_transform();
        },
        &params(1.0, UVec2::new(400, 400)),
    );
    assert_eq!(buf.texts.len(), 1);
    assert_eq!(buf.texts[0].scale, 2.0);
}

#[test]
fn compose_composes_nested_transforms() {
    let buf = run(
        |b, _arena| {
            b.push_transform(TranslateScale::new(Vec2::new(3.0, 5.0), 2.0));
            b.push_transform(TranslateScale::new(Vec2::new(7.0, 11.0), 4.0));
            draw(b, Rect::new(-2.0, 3.0, 4.0, 5.0));
            b.pop_transform();
            b.pop_transform();
        },
        &params(1.0, UVec2::new(400, 400)),
    );
    let q = &buf.quads.instance()[0];
    assert_eq!(q.header.rect.min, Vec2::new(1.0, 51.0));
    assert_eq!(q.header.rect.size, Size::new(32.0, 40.0));
}

#[test]
fn compose_transforms_clip_rects_to_screen_space() {
    let buf = run(
        |b, _arena| {
            b.push_transform(TranslateScale::from_scale(2.0));
            clip(b, Rect::new(10.0, 10.0, 20.0, 20.0));
            draw(b, Rect::new(15.0, 15.0, 5.0, 5.0));
            b.pop_clip();
            b.pop_transform();
        },
        &params(1.0, UVec2::new(400, 400)),
    );
    assert_eq!(
        quad_runs(&buf),
        [(Span::new(0, 1), Some(URect::new(20, 20, 40, 40)))]
    );
}

/// The composer applies the display scale, runs the icon size ladder, and
/// lands the quad on whole pixels. Hand-computed at 1.5, where a 24 px icon is
/// 36 physical and nothing divides evenly.
#[test]
fn icon_resolves_to_a_whole_pixel_raster_at_the_display_scale() {
    use crate::renderer::frontend::composer::tests::support::icon;
    use glam::{IVec2, U16Vec2};

    // 24 logical px at 1.5 → 36 physical, inside the exact band.
    let out = run(
        |buf, _| {
            icon(
                buf,
                Rect::new(10.0, 20.0, 24.0, 24.0),
                IconRef::fixture(0, 3),
            );
        },
        &params(1.5, UVec2::new(200, 200)),
    );
    assert_eq!(out.icons.len(), 1, "one row per icon");
    let row = out.icons[0];
    assert_eq!(row.key.size(), U16Vec2::new(36, 36));
    assert_eq!(
        row.key.icon,
        IconRef::fixture(0, 3),
        "identity survives compose"
    );
    // Origin 10*1.5 = 15, 20*1.5 = 30, and the raster fills the box exactly,
    // so centering shifts nothing.
    assert_eq!(row.origin, IVec2::new(15, 30));
    assert_eq!(row.size, row.key.size().as_uvec2(), "drawn texel for texel");
    assert_eq!(
        rows_of(&out, Lane::Tier(PaintTier::Icon)).len(),
        1,
        "icons in one group coalesce into one run, and so one draw",
    );

    // 50 logical px at 1.5 -> 75 physical, past the exact band: the ladder
    // rounds the raster up to 76; the quad is the 75 px box at (15, 15).
    let out = run(
        |buf, _| {
            icon(
                buf,
                Rect::new(10.0, 10.0, 50.0, 50.0),
                IconRef::fixture(0, 0),
            );
        },
        &params(1.5, UVec2::new(200, 200)),
    );
    assert_eq!(out.icons[0].key.size(), U16Vec2::new(76, 76));
    assert_eq!(out.icons[0].origin, IVec2::new(15, 15));
    assert_eq!(out.icons[0].size, UVec2::new(75, 75));

    // 300 logical px at 2 → 600 physical, past the 512 cap: the raster is
    // 512, and the quad still fills the 600 px box at (20, 40).
    let out = run(
        |buf, _| {
            icon(
                buf,
                Rect::new(10.0, 20.0, 300.0, 300.0),
                IconRef::fixture(0, 0),
            );
        },
        &params(2.0, UVec2::new(800, 800)),
    );
    assert_eq!(out.icons[0].key.size(), U16Vec2::new(512, 512));
    assert_eq!(out.icons[0].origin, IVec2::new(20, 40));
    assert_eq!(out.icons[0].size, UVec2::new(600, 600));

    // A box past 65535 px, as a zoom makes: (−90000, 0) wide 100000 still covers the screen to
    // 10000, and the quad is the whole box, where 16 bits held 65535 and ended it at −24465.
    let out = run(
        |buf, _| {
            icon(
                buf,
                Rect::new(-90000.0, 0.0, 100000.0, 100.0),
                IconRef::fixture(0, 0),
            );
        },
        &params(1.0, UVec2::new(200, 200)),
    );
    assert_eq!(out.icons[0].origin, IVec2::new(-90000, 0));
    assert_eq!(out.icons[0].size, UVec2::new(100000, 100));
}

/// Two icons in one group share a batch; an overlapping curve above them
/// splits it.
#[test]
fn icons_batch_together_and_respect_tier_order() {
    use crate::renderer::frontend::composer::tests::support::icon;

    let out = run(
        |buf, _| {
            icon(buf, Rect::new(0.0, 0.0, 16.0, 16.0), IconRef::fixture(0, 0));
            icon(
                buf,
                Rect::new(20.0, 0.0, 16.0, 16.0),
                IconRef::fixture(0, 1),
            );
        },
        &params(1.0, UVec2::new(200, 200)),
    );
    assert_eq!(out.icons.len(), 2);
    assert_eq!(
        rows_of(&out, Lane::Tier(PaintTier::Icon)),
        [Span::new(0, 2)],
        "two icons, one draw"
    );
}

/// A triangle reaches the GPU as its points grown by its radius, from the covering quad's
/// top-left in f32, clockwise on screen, a radius at each, and its inradius: the template takes each
/// corner's arc center back as the grown point less the radius along the corner's bisector, so the
/// centers come back to the points within f32 rounding of a 3000 px quad.
///
/// The points are given counterclockwise on screen, `(b − a) × (c − a) < 0`, so the quad swaps the
/// last two. The covering quad is the points' bounds grown by the corner radius alone, since the
/// shader grows every quad by its AA ramp: (13.3, 7.1) to (3013.7, 2950.6), less and plus 2. The
/// inradius is the triangle's own, `|cross| / perimeter`, plus the radius 2.
#[test]
fn a_wide_triangle_keeps_its_corners_to_a_fraction_of_a_pixel() {
    let points = [
        Vec2::new(13.3, 7.1),
        Vec2::new(1500.2, 2950.6),
        Vec2::new(3013.7, 41.9),
    ];
    let buf = run(
        |b, _arena| {
            b.draw(
                DrawClosedPayload::new(
                    PlacedClosed::Triangle(PlacedTriangle {
                        centers: points,
                        radius: 2.0,
                    }),
                    ClosedFill::new(
                        BrushSource::Solid(RgbaF32::WHITE.to_f16()),
                        LoweredBorder::lower(Border::NONE),
                    ),
                ),
                1.0,
            );
        },
        &params(1.0, UVec2::new(4000, 4000)),
    );
    assert_eq!(buf.quads.len(), 1);
    let quad = buf.quads.instance()[0];
    assert_eq!(
        quad.header.rect,
        Rect::from_min_max(Vec2::new(11.3, 5.1), Vec2::new(3015.7, 2952.6)),
    );
    assert_eq!(quad.header.corners, Corners::new(2.0, 2.0, 2.0, 0.0));
    let [a, b, c] = points;
    let cross = (b - a).perp_dot(c - a);
    assert!(cross < 0.0, "given counterclockwise");
    let perimeter = (b - a).length() + (c - b).length() + (a - c).length();
    assert_eq!(quad.header.inradius, cross.abs() / perimeter + 2.0);
    // Clockwise, as the quad holds them: the last two swapped.
    let clockwise = [a, c, b];
    let grown: [Vec2; 3] = std::array::from_fn(|i| quad.header.rect.min + quad.header.points[i]);
    assert_eq!(
        quad.header.points[3], quad.header.points[2],
        "the fourth point repeats the third"
    );
    let normal = |p: Vec2, q: Vec2| {
        let d = q - p;
        Vec2::new(d.y, -d.x) / d.length()
    };
    for i in 0..3 {
        let na = normal(grown[(i + 2) % 3], grown[i]);
        let nb = normal(grown[i], grown[(i + 1) % 3]);
        let center = grown[i] - 2.0 * (na + nb) / (1.0 + na.dot(nb));
        assert!(
            center.abs_diff_eq(clockwise[i], 1e-3),
            "corner {i}: center {center}, point {}",
            clockwise[i]
        );
    }
}

/// A triangle turns as a rect does: its quad moves so its center turns about the pivot, and the
/// shader turns it about that center, so its points stay in the quad's own frame and its gradient
/// turns with it. At scale 2 and radius 1 the quad of (4, 4), (14, 4), (4, 14) is (6, 6) to
/// (30, 30), its center (18, 18); a quarter turn about the pivot (14, 14), physical (28, 28),
/// `(x, y) ↦ (28 − (y − 28), 28 + (x − 28))`, takes the center to (38, 18), so the quad is
/// (26, 6) to (50, 30), turned by π/2. The tolerance is the f32 cosine of a quarter turn, −4.4e-8,
/// not 0, times the lengths involved.
#[test]
fn a_turned_triangle_turns_its_quad_about_its_pivot() {
    const EPS: f32 = 1e-4;
    let quad = |spin: Option<Spin>| {
        let buf = run(
            |b, _| {
                b.draw(
                    DrawClosedPayload::new(
                        PlacedClosed::Triangle(PlacedTriangle {
                            centers: [
                                Vec2::new(4.0, 4.0),
                                Vec2::new(14.0, 4.0),
                                Vec2::new(4.0, 14.0),
                            ],
                            radius: 1.0,
                        }),
                        ClosedFill::new(
                            BrushSource::Solid(RgbaF32::WHITE.to_f16()),
                            LoweredBorder::lower(Border::NONE),
                        ),
                    )
                    .turned(spin),
                    1.0,
                );
            },
            &params(2.0, UVec2::new(100, 100)),
        );
        buf.quads.instance()[0]
    };
    let still = quad(None);
    assert_eq!(
        still.header.rect,
        Rect::from_min_max(Vec2::new(6.0, 6.0), Vec2::new(30.0, 30.0))
    );
    assert_eq!(still.header.rotation, 0.0);
    let turned = quad(Some(Spin {
        pivot: Vec2::new(14.0, 14.0),
        angle: FRAC_PI_2,
    }));
    assert!(
        turned
            .header
            .rect
            .min
            .abs_diff_eq(Vec2::new(26.0, 6.0), EPS)
            && turned
                .header
                .rect
                .max()
                .abs_diff_eq(Vec2::new(50.0, 30.0), EPS),
        "{:?}",
        turned.header.rect
    );
    assert_eq!(turned.header.rotation, FRAC_PI_2);
    assert_eq!(turned.header.points, still.header.points);
    assert_eq!(turned.header.inradius, still.header.inradius);
}

/// A triangle's gradient reaches its quad as a rect's does: the kind with the triangle bit, the
/// atlas row, and the axis as given at scale 2, as it is a fraction of the quad.
#[test]
fn a_triangle_carries_its_gradient_lanes() {
    let axis = FillAxis::from_lanes([0.25, 0.5, 0.75, 1.0]);
    let buf = run(
        |b, _| {
            b.draw(
                DrawClosedPayload::new(
                    PlacedClosed::Triangle(PlacedTriangle {
                        centers: [
                            Vec2::new(4.0, 4.0),
                            Vec2::new(14.0, 4.0),
                            Vec2::new(4.0, 14.0),
                        ],
                        radius: 1.0,
                    }),
                    ClosedFill::new(
                        BrushSource::Gradient(ResolvedGradient {
                            axis,
                            lut_row: LutRow(3),
                            kind: FillKind::radial(Spread::Repeat),
                        }),
                        LoweredBorder::lower(Border::NONE),
                    ),
                ),
                1.0,
            );
        },
        &params(2.0, UVec2::new(100, 100)),
    );
    let quad = buf.quads.instance()[0];
    assert_eq!(
        buf.quads.key(),
        [QuadKey::Fill(FillForm::new(
            ShapeForm::Triangle,
            FillBrush::Gradient
        ))]
    );
    assert_eq!(quad.fill_kind, FillKind::radial(Spread::Repeat));
    assert_eq!(quad.fill_lut_row, LutRow(3));
    assert_eq!(quad.fill_axis, axis);
    assert_eq!(quad.fill, RgbaF16::WHITE);
}

/// The display's pixel snap reaches the composed quad: a fractional rect at
/// DPR 1.5 lands on whole pixels with snap on and keeps quarter-pixel edges
/// with it off (see `Rect::scaled_by`).
///
/// A transform places the snapped rect exactly, as Blink applies a transform after it snaps
/// layout: under a (0.25, 0.5) translation, (15, 16, 31, 8) moves by (0.375, 0.75) physical px to
/// (15.375, 16.75, 31, 8); under a 2× scale it grows to (30, 32, 62, 16). Snapped after the
/// transform it read (16, 17, 31, 8) and (31, 32, 61, 16), and a slow translation stood still.
#[test]
fn compose_snaps_quad_edges_only_under_pixel_snap() {
    let snapped = params(1.5, UVec2::new(400, 400));
    for (display, transform, want) in [
        (snapped, None, Rect::new(15.0, 16.0, 31.0, 8.0)),
        (
            params_unsnapped(1.5, UVec2::new(400, 400)),
            None,
            Rect::new(15.375, 16.125, 30.75, 7.875),
        ),
        (
            snapped,
            Some(TranslateScale::from_translation(Vec2::new(0.25, 0.5))),
            Rect::new(15.375, 16.75, 31.0, 8.0),
        ),
        (
            snapped,
            Some(TranslateScale::from_scale(2.0)),
            Rect::new(30.0, 32.0, 62.0, 16.0),
        ),
    ] {
        let buf = run(
            |b, _| {
                if let Some(transform) = transform {
                    b.push_transform(transform);
                }
                draw(b, Rect::new(10.25, 10.75, 20.5, 5.25));
            },
            &display,
        );
        assert_eq!(buf.quads.len(), 1);
        assert_eq!(
            buf.quads.instance()[0].header.rect,
            want,
            "snap {}, {transform:?}",
            display.pixel_snap
        );
    }
}

/// A drop shadow's quad is its source snapped as a fill is, then moved by
/// the offset and grown by the halo, both in physical px. At 1.5× the
/// source (10.25, 10.75, 20.5, 5.25) snaps to (15, 16, 31, 8), as in
/// `compose_snaps_quad_edges_only_under_pixel_snap`. The lanes scale to
/// offset (3, −1.5), σ 3, spread 1.5, so the halo is 4·3 + 1.5 = 13.5:
/// min (15 + 3 − 13.5, 16 − 1.5 − 13.5) = (4.5, 1), size (31 + 27, 8 + 27).
/// The radii fit the source: 4 × 1.5 = 6 per corner, 12 over its 8 px
/// sides, so f = 8 / 12 and each is 4.
///
/// An inset shadow of the same source paints over the snapped source itself, its kind and its
/// scaled lanes in the shadow's own fields.
///
/// A source with no size still casts its spread: (50, 50) grown by 2 is
/// (48, 48, 4, 4). With no blur and no spread it casts nothing.
///
/// A σ of 40000 at 2× is 80000 physical px, past the largest f16: the lane
/// saturates at 65504 and the quad grows by 4 · 65504 = 262016 from the
/// source (20, 20, 40, 40), to (−261996, −261996, 524072, 524072), where an
/// infinite lane would have dropped it.
#[test]
fn drop_shadow_quad_grows_from_the_snapped_source() {
    use crate::primitives::paint::shadow::Shadow;

    let drop = |source: Rect, corners: f32, lanes: [f32; 4]| {
        let [x, y, blur, spread] = lanes;
        let shadow = Shadow::drop(RgbaF32::srgba(0.0, 0.0, 0.0, 0.5), Vec2::new(x, y), blur)
            .with_spread(spread);
        DrawShadowPayload::new(
            PlacedBox {
                rect: source,
                corners: Corners::all(corners),
            },
            LoweredShadow::lower(shadow),
        )
    };
    let buf = run(
        |b, _| {
            b.draw(
                drop(
                    Rect::new(10.25, 10.75, 20.5, 5.25),
                    4.0,
                    [2.0, -1.0, 2.0, 1.0],
                ),
                1.0,
            );
        },
        &params(1.5, UVec2::new(400, 400)),
    );
    assert_eq!(buf.quads.key(), [QuadKey::Shadow]);
    let q = ShadowInstance::from(buf.quads.instance()[0]);
    assert_eq!(q.header.rect, Rect::new(4.5, 1.0, 58.0, 35.0));
    assert_eq!(q.header.corners, Corners::all(4.0));
    assert_eq!(q.kind, ShadowKind::DROP);
    assert_eq!(q.geometry.lanes(), [3.0, -1.5, 3.0, 1.5]);

    let buf = run(
        |b, _| {
            let mut p = drop(
                Rect::new(10.25, 10.75, 20.5, 5.25),
                4.0,
                [2.0, -1.0, 2.0, 1.0],
            );
            p.shadow = LoweredShadow::lower(
                Shadow::inset(
                    RgbaF32::srgba(0.0, 0.0, 0.0, 0.5),
                    Vec2::new(2.0, -1.0),
                    2.0,
                )
                .with_spread(1.0),
            );
            b.draw(p, 1.0);
        },
        &params(1.5, UVec2::new(400, 400)),
    );
    assert_eq!(buf.quads.key(), [QuadKey::Shadow]);
    let q = ShadowInstance::from(buf.quads.instance()[0]);
    assert_eq!(
        q.header.rect,
        Rect::new(15.0, 16.0, 31.0, 8.0),
        "the snapped source"
    );
    assert_eq!(q.header.corners, Corners::all(4.0));
    assert_eq!(q.kind, ShadowKind::INSET);
    assert_eq!(q.geometry.lanes(), [3.0, -1.5, 3.0, 1.5]);

    let point = Rect::new(50.0, 50.0, 0.0, 0.0);
    let buf = run(
        |b, _| {
            b.draw(drop(point, 0.0, [0.0, 0.0, 0.0, 2.0]), 1.0);
            b.draw(drop(point, 0.0, [0.0, 0.0, 0.0, 0.0]), 1.0);
        },
        &params(1.0, UVec2::new(200, 200)),
    );
    assert_eq!(buf.quads.len(), 1, "only the spread one paints");
    assert_eq!(
        buf.quads.instance()[0].header.rect,
        Rect::new(48.0, 48.0, 4.0, 4.0)
    );

    let buf = run(
        |b, _| {
            b.draw(
                drop(
                    Rect::new(10.0, 10.0, 20.0, 20.0),
                    0.0,
                    [0.0, 0.0, 40000.0, 0.0],
                ),
                1.0,
            );
        },
        &params(2.0, UVec2::new(200, 200)),
    );
    assert_eq!(buf.quads.len(), 1, "the widest shadow still paints");
    let q = ShadowInstance::from(buf.quads.instance()[0]);
    assert_eq!(q.geometry.lanes(), [0.0, 0.0, 65504.0, 0.0]);
    assert_eq!(
        q.header.rect,
        Rect::new(-261996.0, -261996.0, 524072.0, 524072.0)
    );
}

/// A mesh at origin (4, 4), at display scale 2, its instance `x_axis`, `y_axis` and `translate`.
/// Unturned and unfit: `(2, 0)`, `(0, 2)` and `2·(4, 4) = (8, 8)`. A quarter turn about the pivot
/// (10, 10) of the logical frame, `c = (6, 6)` from the origin, maps `p` to `R(p − c) + c`, `R c = (−6, 6)`: the axes `2·(cos, sin) = (0, 2)` and
/// `2·(−sin, cos) = (−2, 0)`, the translate `2·((4, 4) + c − R c) = 2·(16, 4) = (32, 8)`. The fit
/// `p ↦ (1, 2) + (3, 0.5)·p` scales each axis, `(6, 0)` and `(0, 1)`, and moves the translate to
/// `2·((4, 4) + (1, 2)) = (10, 12)`; turned as well, `R((1, 2) − c) + c = R(−5, −4) + c =
/// (4, −5) + (6, 6) = (10, 1)`, so `2·((4, 4) + (10, 1)) = (28, 10)`, and the axes `(0, 6)` and
/// `(−1, 0)`. The tolerance is the f32 cosine of a quarter turn, −4.4e-8, not 0, times the
/// lengths involved.
#[test]
fn a_mesh_instance_fits_turns_and_scales_at_once() {
    const EPS: f32 = 1e-5;
    let instance = |spin: Option<Spin>, fit: FitMap| {
        let buf = run(
            |b, _| {
                b.draw(
                    DrawMeshPayload {
                        bbox: Rect::new(0.0, 0.0, 12.0, 12.0),
                        origin: Vec2::new(4.0, 4.0),
                        fit,
                        tint: RgbaF32::WHITE.to_f16(),
                        vertices: Span::new(0, 3),
                        idxs: Span::new(0, 3),
                        spin,
                    },
                    1.0,
                );
            },
            &params(2.0, UVec2::new(200, 200)),
        );
        let instance = buf.meshes.instance()[0];
        [instance.x_axis, instance.y_axis, instance.translate]
    };
    let quarter = Some(Spin {
        pivot: Vec2::new(10.0, 10.0),
        angle: FRAC_PI_2,
    });
    let fit = FitMap {
        offset: Vec2::new(1.0, 2.0),
        scale: Vec2::new(3.0, 0.5),
    };
    let close = |got: [Vec2; 3], want: [Vec2; 3]| {
        assert!(
            got.iter().zip(want).all(|(g, w)| g.abs_diff_eq(w, EPS)),
            "{got:?} != {want:?}"
        );
    };
    assert_eq!(
        instance(None, FitMap::IDENTITY),
        [
            Vec2::new(2.0, 0.0),
            Vec2::new(0.0, 2.0),
            Vec2::new(8.0, 8.0)
        ]
    );
    close(
        instance(quarter, FitMap::IDENTITY),
        [
            Vec2::new(0.0, 2.0),
            Vec2::new(-2.0, 0.0),
            Vec2::new(32.0, 8.0),
        ],
    );
    assert_eq!(
        instance(None, fit),
        [
            Vec2::new(6.0, 0.0),
            Vec2::new(0.0, 1.0),
            Vec2::new(10.0, 12.0)
        ]
    );
    close(
        instance(quarter, fit),
        [
            Vec2::new(0.0, 6.0),
            Vec2::new(-1.0, 0.0),
            Vec2::new(28.0, 10.0),
        ],
    );
}

/// An opaque rect turned a quarter about the logical pivot (50, 50), at display scale 2: its
/// physical rect (80, 80)..(120, 100) has its center (100, 90) turned about the physical pivot
/// (100, 100) to (110, 100), so the instance is (90, 90) 40×20 at the angle, unsnapped. A turned
/// quad occludes nothing: a quad under its own rect but outside the turned shape, at physical
/// (124, 94)..(126, 96), survives the group.
#[test]
fn a_turned_rect_moves_its_center_and_keeps_its_own_frame() {
    const EPS: f32 = 1e-4;
    let display = params(2.0, UVec2::new(200, 200));
    let buf = run(
        |b, _| {
            QuadBuilder::new(Rect::new(62.0, 47.0, 1.0, 1.0)).draw(b);
            QuadBuilder::new(Rect::new(40.0, 40.0, 20.0, 10.0))
                .turned_about(Vec2::new(50.0, 50.0), FRAC_PI_2)
                .draw(b);
        },
        &display,
    );
    assert_eq!(buf.quads.len(), 2, "the turned quad occludes nothing");
    let turned = buf.quads.instance()[1];
    assert!(
        turned
            .header
            .rect
            .min
            .abs_diff_eq(Vec2::new(90.0, 90.0), EPS),
        "{:?}",
        turned.header.rect
    );
    assert_eq!(turned.header.rect.size, Size::new(40.0, 20.0));
    assert_eq!(turned.header.rotation, FRAC_PI_2);
}

/// A shaped image's quad is its shape's rect, with the image's uv carried over the whole rect and
/// its own rect as the decal. At scale 2 the shape (5, 5, 50 × 25) is (10, 10, 100 × 50) and the
/// contained image (17.5, 5, 25 × 25) is (35, 10, 50 × 50), 25 px in: one uv per 50 px, so the
/// rect's uv starts 25 / 50 = 0.5 before the image's and spans 100 / 50 = 2 by 1. The decal is
/// x 25 to 75, and y open, as the image's top and bottom lie on the shape's. Corners of 4 scale
/// to 8, the inradius is half the short side, 25. A turn moves no field but the rotation. An image
/// the shape's own size keeps its uv and an open decal.
#[test]
fn a_shaped_image_carries_its_uv_and_decal_over_its_shape() {
    let shaped = |image: Rect, spin: Option<Spin>| {
        let buf = run(
            |b, _| {
                b.draw(
                    DrawClosedPayload {
                        geometry: PlacedClosed::Box(PlacedBox {
                            rect: Rect::new(5.0, 5.0, 50.0, 25.0),
                            corners: Corners::all(4.0),
                        }),
                        spin,
                        paint: ClosedImage {
                            rect: image,
                            uv_min: Vec2::ZERO,
                            uv_size: Vec2::ONE,
                            tint: RgbaF32::WHITE.to_f16(),
                            handle: TextureId(1),
                            flags: ImageFlags::TILED,
                            view: None,
                        },
                    },
                    1.0,
                );
            },
            &params(2.0, UVec2::new(200, 200)),
        );
        assert_eq!(buf.images.key(), [ShapeForm::Box]);
        buf.images.instance()[0]
    };
    let contained = shaped(Rect::new(17.5, 5.0, 25.0, 25.0), None);
    assert_eq!(contained.header.rect, Rect::new(10.0, 10.0, 100.0, 50.0));
    assert_eq!(contained.uv_min, Vec2::new(-0.5, 0.0));
    assert_eq!(contained.uv_size, Vec2::new(2.0, 1.0));
    assert_eq!(contained.decal_min, Vec2::new(25.0, -f32::MAX));
    assert_eq!(contained.decal_max, Vec2::new(75.0, f32::MAX));
    assert_eq!(contained.flags, ImageFlags::TILED);
    assert_eq!(contained.header.corners, Corners::all(8.0));
    assert_eq!(
        contained.header.points,
        [
            Vec2::ZERO,
            Vec2::new(100.0, 0.0),
            Vec2::new(100.0, 50.0),
            Vec2::new(0.0, 50.0)
        ]
    );
    assert_eq!(contained.header.inradius, 25.0);
    assert_eq!(contained.header.rotation, 0.0);

    // Turned about the shape's center, logical (30, 17.5), the quad stays and turns in the
    // shader, the uv and the decal in its own frame.
    let turned = shaped(
        Rect::new(17.5, 5.0, 25.0, 25.0),
        Some(Spin {
            pivot: Vec2::new(30.0, 17.5),
            angle: FRAC_PI_2,
        }),
    );
    assert_eq!(turned.header.rect, contained.header.rect);
    assert_eq!(turned.header.rotation, FRAC_PI_2);
    assert_eq!(
        (
            turned.uv_min,
            turned.uv_size,
            turned.decal_min,
            turned.decal_max
        ),
        (
            contained.uv_min,
            contained.uv_size,
            contained.decal_min,
            contained.decal_max
        )
    );

    let filled = shaped(Rect::new(5.0, 5.0, 50.0, 25.0), None);
    assert_eq!(filled.uv_min, Vec2::ZERO);
    assert_eq!(filled.uv_size, Vec2::ONE);
    assert_eq!(filled.decal_min, Vec2::splat(-f32::MAX));
    assert_eq!(filled.decal_max, Vec2::splat(f32::MAX));
}

/// A triangle's image packs its shape as a triangle quad does, unsnapped: at scale 2 the corners
/// (5, 5), (25, 5), (5, 15) with radius 1 cover (8, 8) to (52, 32), and the image fills that
/// rect, so its uv stays and its decal is open. It draws through the triangle form.
#[test]
fn a_triangle_image_packs_its_shape_as_a_triangle_quad_does() {
    let points = [
        Vec2::new(5.0, 5.0),
        Vec2::new(25.0, 5.0),
        Vec2::new(5.0, 15.0),
    ];
    let buf = run(
        |b, _| {
            b.draw(
                DrawClosedPayload {
                    geometry: PlacedClosed::Triangle(PlacedTriangle {
                        centers: points,
                        radius: 1.0,
                    }),
                    spin: None,
                    paint: ClosedImage {
                        rect: Rect::new(4.0, 4.0, 22.0, 12.0),
                        uv_min: Vec2::ZERO,
                        uv_size: Vec2::ONE,
                        tint: RgbaF32::WHITE.to_f16(),
                        handle: TextureId(1),
                        flags: ImageFlags::NONE,
                        view: None,
                    },
                },
                1.0,
            );
            b.draw(
                DrawClosedPayload::new(
                    PlacedClosed::Triangle(PlacedTriangle {
                        centers: points,
                        radius: 1.0,
                    }),
                    ClosedFill::new(
                        BrushSource::Solid(RgbaF32::WHITE.to_f16()),
                        LoweredBorder::lower(Border::NONE),
                    ),
                ),
                1.0,
            );
        },
        &params(2.0, UVec2::new(200, 200)),
    );
    assert_eq!(buf.images.key(), [ShapeForm::Triangle]);
    let image = buf.images.instance()[0];
    let quad = buf.quads.instance()[0];
    assert_eq!(
        image.header.rect,
        Rect::from_min_max(Vec2::new(8.0, 8.0), Vec2::new(52.0, 32.0))
    );
    assert_eq!(image.header.rect, quad.header.rect);
    assert_eq!(image.header.points, quad.header.points);
    assert_eq!(image.header.corners, quad.header.corners);
    assert_eq!(image.header.inradius, quad.header.inradius);
    assert_eq!(image.uv_min, Vec2::ZERO);
    assert_eq!(image.uv_size, Vec2::ONE);
    assert_eq!(image.decal_min, Vec2::splat(-f32::MAX));
    assert_eq!(image.decal_max, Vec2::splat(f32::MAX));
}

/// An image edge on its shape's edge leaves the shape to fade it, and one inside it fades on its
/// own. A triangle's image filling its bounds keeps an open decal at a scale of 1.25, where its
/// rect, fit in logical px, and its shape's, from the scaled points, part by f32 rounding. A
/// turned rect at scale 2, edges exact: an image 1/64 in on the left is 1/32 physical px in, past
/// the slack, so its decal starts at 0.03125; 1/256 in on the right is 1/128 in, within it, so
/// that side stays open, as do the top and bottom on the shape's.
#[test]
fn an_image_edge_on_its_shape_leaves_the_fade_to_the_shape() {
    let draw = |rect: Rect, shape: PlacedClosed, spin: Option<Spin>, scale: f32| {
        let buf = run(
            |b, _| {
                b.draw(
                    DrawClosedPayload {
                        geometry: shape,
                        spin,
                        paint: ClosedImage {
                            rect,
                            uv_min: Vec2::ZERO,
                            uv_size: Vec2::ONE,
                            tint: RgbaF32::WHITE.to_f16(),
                            handle: TextureId(1),
                            flags: ImageFlags::NONE,
                            view: None,
                        },
                    },
                    1.0,
                );
            },
            &params(scale, UVec2::new(200, 200)),
        );
        buf.images.instance()[0]
    };
    for i in 0..16 {
        let f = i as f32 * 0.137;
        let shape = PlacedClosed::Triangle(PlacedTriangle {
            centers: [
                Vec2::new(5.1 + f, 5.3),
                Vec2::new(25.7 + f, 5.9 + f),
                Vec2::new(5.3, 15.1 + f),
            ],
            radius: 1.3 + f * 0.1,
        });
        let image = draw(shape.bounds(), shape, None, 1.25);
        assert_eq!(image.decal_min, Vec2::splat(-f32::MAX), "triangle {i}");
        assert_eq!(image.decal_max, Vec2::splat(f32::MAX), "triangle {i}");
    }
    let shape = PlacedClosed::Box(PlacedBox {
        rect: Rect::new(5.0, 5.0, 50.0, 25.0),
        corners: Corners::all(4.0),
    });
    let image = draw(
        Rect::from_min_max(
            Vec2::new(5.0 + 1.0 / 64.0, 5.0),
            Vec2::new(55.0 - 1.0 / 256.0, 30.0),
        ),
        shape,
        Some(Spin {
            pivot: Vec2::new(30.0, 17.5),
            angle: FRAC_PI_2,
        }),
        2.0,
    );
    assert_eq!(image.decal_min, Vec2::new(0.03125, -f32::MAX));
    assert_eq!(image.decal_max, Vec2::splat(f32::MAX));
}
