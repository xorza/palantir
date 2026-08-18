//! What the display scale and a transform do to what is drawn.

use crate::primitives::{
    color::Color, corners::Corners, size::Size, stroke::Stroke, transform::TranslateScale,
    urect::URect,
};
use crate::renderer::frontend::composer::geometry::stroke_bbox_urect;
use crate::renderer::frontend::composer::tests::support::{clip, draw, params, rect, run, text};
use crate::renderer::frontend::paint_sink::PaintSink;
use crate::renderer::frontend::payload::{BrushSource, DrawQuadPayload};
use crate::scene::shapes::paint::ShapeStroke;
use crate::shape::style::{LineCap, LineJoin};
use glam::{UVec2, Vec2};

#[test]
fn stroke_bbox_urect_applies_transform_dpi_and_style_once() {
    #[derive(Debug)]
    struct Case {
        scale: f32,
        cap: LineCap,
        join: Option<LineJoin>,
        expected: URect,
    }

    // Centerline (10,20)..(30,30), plus origin (2,4), then
    // x ↦ 1.5x + (3,5) gives logical (21,41)..(51,56).
    // Butt cases use physical pad = width_phys/2 + 0.5:
    // 0.5× → 2, 1× → 3.5, 2× → 6.5.
    let cases = [
        Case {
            scale: 0.5,
            cap: LineCap::Butt,
            join: None,
            expected: URect::new(8, 18, 20, 12),
        },
        Case {
            scale: 1.0,
            cap: LineCap::Butt,
            join: None,
            expected: URect::new(17, 37, 38, 23),
        },
        Case {
            scale: 2.0,
            cap: LineCap::Butt,
            join: None,
            expected: URect::new(35, 75, 74, 44),
        },
        // At 1×, Square pad = 3.5√2 ≈ 4.9498.
        Case {
            scale: 1.0,
            cap: LineCap::Square,
            join: None,
            expected: URect::new(16, 36, 40, 25),
        },
        // At 1×, Miter pad = 3.5·4 = 14.
        Case {
            scale: 1.0,
            cap: LineCap::Butt,
            join: Some(LineJoin::Miter),
            expected: URect::new(7, 27, 58, 43),
        },
    ];
    let xform = TranslateScale::new(Vec2::new(3.0, 5.0), 1.5);

    for case in cases {
        let actual = stroke_bbox_urect(
            xform,
            rect(10.0, 20.0, 20.0, 10.0),
            Vec2::new(2.0, 4.0),
            4.0 * 1.5 * case.scale,
            case.cap,
            case.join,
            params(case.scale, UVec2::new(200, 200)),
        );
        assert_eq!(actual, case.expected, "{case:?}");
    }
}

/// A NaN stroke width normalizes away like any other non-painting
/// width, uniformly for every quad shape. `Shape::debug_assert_no_nan`
/// is what catches it loudly, at the authoring boundary; this pins the
/// release-side fallback, which is to fail safe.
///
/// Pinned end to end rather than at the payload, because the interesting
/// claim is about what reaches the GPU: **no NaN ever does**, on either
/// geometry. Before `ShapeStroke` carried an `f32` width the two arms
/// disagreed here — rect forwarded NaN to the instance, triangle scrubbed
/// it via `.max(0.0)` — and nothing was checking that they agreed.
#[test]
fn nan_stroke_width_normalizes_away_on_every_quad_geometry() {
    let nan_stroke: ShapeStroke = Stroke::solid(Color::rgb(0.0, 1.0, 0.0), f32::NAN).into();
    let display = params(2.0, UVec2::new(400, 400));

    // An opaque fill keeps the draw alive, so the quad reaches the
    // buffer and its stroke lanes can be inspected. With a transparent
    // fill the whole payload gates out instead — also fine, but it
    // proves nothing about the lanes.
    let buf = run(
        |b, _arena| {
            b.draw_quad(DrawQuadPayload::rect(
                rect(10.0, 20.0, 30.0, 40.0),
                Corners::ZERO,
                BrushSource::Solid(Color::WHITE.into()),
                nan_stroke,
            ));
        },
        &display,
    );
    assert_eq!(buf.quads.len(), 1, "the fill keeps the rect alive");
    assert_eq!(
        buf.quads[0].stroke_width, 0.0,
        "a NaN width must not reach the instance",
    );

    let buf = run(
        |b, _arena| {
            b.draw_quad(DrawQuadPayload::triangle(
                Vec2::ZERO,
                [
                    Vec2::new(0.0, 0.0),
                    Vec2::new(10.0, 0.0),
                    Vec2::new(5.0, 8.0),
                ],
                Color::WHITE.into(),
                0.0,
                nan_stroke,
            ));
        },
        &display,
    );
    assert_eq!(buf.quads.len(), 1, "the fill keeps the triangle alive");
    assert_eq!(
        buf.quads[0].stroke_width, 0.0,
        "the triangle arm must agree with the rect arm",
    );

    // A NaN stroke on a shape with nothing else to paint is simply
    // dropped — the fill and the stroke are both no-ops.
    let buf = run(
        |b, _arena| {
            b.draw_quad(DrawQuadPayload::rect(
                rect(10.0, 20.0, 30.0, 40.0),
                Corners::ZERO,
                BrushSource::Solid(Color::TRANSPARENT.into()),
                nan_stroke,
            ));
        },
        &display,
    );
    assert!(buf.quads.is_empty(), "nothing to paint, nothing emitted");
}

#[test]
fn compose_scales_rects_for_dpr() {
    let buf = run(
        |b, _arena| draw(b, rect(10.0, 20.0, 30.0, 40.0)),
        &params(2.0, UVec2::new(400, 400)),
    );
    assert_eq!(buf.quads.len(), 1);
    let q = &buf.quads[0];
    assert_eq!(q.rect.min, Vec2::new(20.0, 40.0));
    assert_eq!(q.rect.size, Size::new(60.0, 80.0));
}

#[test]
fn compose_translates_under_push_transform() {
    let buf = run(
        |b, _arena| {
            b.push_transform(TranslateScale::from_translation(Vec2::new(100.0, 50.0)));
            draw(b, rect(10.0, 20.0, 30.0, 40.0));
            b.pop_transform();
        },
        &params(1.0, UVec2::new(400, 400)),
    );
    assert_eq!(buf.quads.len(), 1);
    let q = &buf.quads[0];
    assert_eq!(q.rect.min, Vec2::new(110.0, 70.0));
    assert_eq!(q.rect.size, Size::new(30.0, 40.0));
}

#[test]
fn compose_scales_radius_and_stroke_under_transform() {
    let buf = run(
        |b, _arena| {
            b.push_transform(TranslateScale::from_scale(2.0));
            b.draw_quad(DrawQuadPayload::rect(
                rect(0.0, 0.0, 50.0, 50.0),
                Corners::all(8.0),
                BrushSource::Solid(Color::rgb(1.0, 1.0, 1.0).into()),
                Stroke::solid(Color::rgb(0.0, 0.0, 0.0), 1.5).into(),
            ));
            b.pop_transform();
        },
        &params(1.0, UVec2::new(400, 400)),
    );
    let q = &buf.quads[0];
    assert_eq!(q.rect.size, Size::new(100.0, 100.0));
    assert_eq!(q.corners.as_array()[0], 16.0);
    assert_eq!(q.stroke_width, 3.0);
}

/// Pin: text-run scale snaps to the additive 0.5% ladder so continuous
/// zoom produces stable glyphon cache keys across adjacent frames.
/// Quads (next test) intentionally do not snap — only text quantizes.
#[test]
fn compose_snaps_text_scale_to_discrete_steps() {
    // 1.013 is between 1.010 and 1.015; rounds to 1.015.
    let buf = run(
        |b, _arena| {
            b.push_transform(TranslateScale::from_scale(1.013));
            text(b, rect(0.0, 0.0, 50.0, 20.0));
            b.pop_transform();
        },
        &params(1.0, UVec2::new(400, 400)),
    );
    assert_eq!(buf.texts.len(), 1);
    let s = buf.texts[0].scale;
    assert!(
        (s - 1.015).abs() < 1e-5,
        "1.013 must snap to 1.015, got {s}",
    );
}

/// Pin: a quad pushed under the same fractional transform keeps its
/// continuous scale — only text snaps. Otherwise a zoomed layout
/// would visibly jitter as quad sizes step alongside font cache keys.
#[test]
fn compose_keeps_quad_scale_continuous_under_zoom() {
    let buf = run(
        |b, _arena| {
            b.push_transform(TranslateScale::from_scale(1.013));
            draw(b, rect(0.0, 0.0, 100.0, 50.0));
            b.pop_transform();
        },
        &params(1.0, UVec2::new(400, 400)),
    );
    assert_eq!(buf.quads.len(), 1);
    // 100*1.013 = 101.3; 50*1.013 = 50.65 — preserved, not snapped.
    assert!((buf.quads[0].rect.size.w - 101.3).abs() < 1e-4);
    assert!((buf.quads[0].rect.size.h - 50.65).abs() < 1e-3);
}

#[test]
fn compose_propagates_transform_scale_to_text_runs() {
    // A `TranslateScale(_, 2.0)` ancestor must surface on the emitted
    // TextDrawRow.scale so glyphon paints proportionally larger glyphs.
    // Without this the rect stretches but the glyph rasters stay at
    // the originally-shaped size — visible as text "not zooming" inside
    // a zoomed Scroll viewport.
    let buf = run(
        |b, _arena| {
            b.push_transform(TranslateScale::from_scale(2.0));
            text(b, rect(0.0, 0.0, 50.0, 20.0));
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
            draw(b, rect(-2.0, 3.0, 4.0, 5.0));
            b.pop_transform();
            b.pop_transform();
        },
        &params(1.0, UVec2::new(400, 400)),
    );
    let q = &buf.quads[0];
    assert_eq!(q.rect.min, Vec2::new(1.0, 51.0));
    assert_eq!(q.rect.size, Size::new(32.0, 40.0));
}

#[test]
fn compose_transforms_clip_rects_to_screen_space() {
    let buf = run(
        |b, _arena| {
            b.push_transform(TranslateScale::from_scale(2.0));
            clip(b, rect(10.0, 10.0, 20.0, 20.0));
            draw(b, rect(15.0, 15.0, 5.0, 5.0));
            b.pop_clip();
            b.pop_transform();
        },
        &params(1.0, UVec2::new(400, 400)),
    );
    assert_eq!(buf.groups.len(), 1);
    let s = buf.groups[0]
        .scissor
        .expect("clipped group must have a scissor");
    assert_eq!((s.min.x, s.min.y, s.size.x, s.size.y), (20, 20, 40, 40));
}
