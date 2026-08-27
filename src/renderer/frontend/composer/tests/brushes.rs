//! Fills, images and raster targets: what each emits and what rides with it.

use crate::primitives::brush::gradient::FillAxis;
use crate::primitives::fill_kind::FillKind;
use crate::primitives::lut_row::LutRow;
use crate::primitives::span::Span;
use crate::primitives::texture_id::TextureId;
use crate::primitives::{
    color::Color, color::ColorU8, corners::Corners, rect::Rect, size::Size, stroke::Stroke,
    translate_scale::TranslateScale,
};
use crate::renderer::frontend::capture::PaintCapture;
use crate::renderer::frontend::composer::tests::support::{
    composer, curve, draw, gpu_paint, gpu_view_payload, image, params, rect, render_buffer, run,
    run_with_texture_cap,
};
use crate::renderer::frontend::paint_sink::{PaintGate, PaintSink};
use crate::renderer::frontend::payload::brush_source::BrushSource;
use crate::renderer::frontend::payload::draw_image_payload::DrawImagePayload;
use crate::renderer::frontend::payload::draw_quad_payload::DrawQuadPayload;
use crate::renderer::frontend::payload::push_clip_payload::PushClipPayload;
use crate::renderer::frontend::payload::resolved_gradient::ResolvedGradient;
use crate::renderer::render_buffer::paint_tier::PaintTier;
use crate::scene::record_store::record_payloads::RecordPayloads;
use glam::{UVec2, Vec2};
use std::time::Duration;

/// Solid `Brush::Solid` panel: composer emits a Quad with
/// `fill_kind = BRUSH_KIND_SOLID = 0`, `fill_lut_row = 0` (sentinel
/// for "no gradient"), and the fill colour pass-through. Catches a
/// regression that accidentally sets `fill_kind = 1` on solid quads.
#[test]
fn compose_solid_brush_emits_kind_zero_quad() {
    let mut buffer = PaintCapture::default();
    buffer.draw_quad(DrawQuadPayload::rect(
        rect(0.0, 0.0, 100.0, 100.0),
        Corners::default(),
        BrushSource::Solid(Color::rgb(0.5, 0.5, 0.5).into()),
        Stroke::ZERO.into(),
    ));
    let mut composer = composer();
    let mut out = render_buffer();
    // 200×200 viewport: an opaque solid sharp quad covering the whole
    // viewport would fold into the clear instead of emitting a quad.
    composer
        .begin(
            params(1.0, UVec2::new(200, 200)),
            Duration::ZERO,
            &RecordPayloads::default(),
            &mut out,
        )
        .replay_from(&buffer);
    let q = &out.quads[0];
    assert_eq!(
        q.fill_kind,
        // Sharp + stroke-less + pixel-aligned, so the solid kind also
        // carries the fragment fast-path bit.
        FillKind::SOLID.with_fast(),
        "solid quad must carry kind=solid (+fast)",
    );
    assert_eq!(
        q.fill_lut_row,
        LutRow::FALLBACK,
        "solid quad has no LUT row",
    );
    assert_eq!(q.fill_axis, FillAxis::ZERO, "solid quad axis is zeroed",);
}

/// A windowed rect must never fold into the pass clear, take the
/// fragment fast path, or occlude quads beneath it — its interior is
/// a hole. All three opaque-cover optimizations compare
/// `fill_kind == FillKind::SOLID` exactly; the window bit breaks that
/// equality by design. Deliberate worst case: full-viewport, opaque,
/// solid, sharp-cornered, pixel-aligned at scale 1 — without the
/// window bit this exact draw would trigger all three.
#[test]
fn windowed_rect_is_not_an_opaque_cover() {
    use crate::primitives::fill_kind::FillKind;
    let buf = run(
        |b, _| {
            draw(b, rect(10.0, 10.0, 50.0, 50.0));
            b.draw_quad(DrawQuadPayload::rect_window(
                rect(0.0, 0.0, 200.0, 200.0),
                Corners::default(),
                BrushSource::Solid(Color::rgb(1.0, 1.0, 1.0).into()),
                Stroke::ZERO.into(),
            ));
        },
        &params(1.0, UVec2::new(200, 200)),
    );
    assert!(
        buf.clear_override.is_none(),
        "windowed cover must not clear-fold",
    );
    assert_eq!(
        buf.quads.len(),
        2,
        "under-quad survives beneath a windowed cover",
    );
    assert_eq!(
        buf.quads[1].fill_kind,
        FillKind::SOLID.with_window(),
        "window bit rides through to the Quad; fast bit absent",
    );
}

/// A resolved linear gradient packs row + axis + kind into the
/// paint payload; composer pipes them through to the emitted Quad.
#[test]
fn compose_linear_brush_emits_kind_one_with_atlas_row() {
    use crate::primitives::brush::gradient::Spread;
    use crate::primitives::brush::gradient::linear_geometry::LinearGradient;
    use crate::primitives::fill_kind::FillKind;
    use crate::renderer::gradient_atlas::shared_gradient_atlas::SharedGradientAtlas;
    let g =
        LinearGradient::two_stop(0.0, ColorU8::WHITE, ColorU8::BLACK).with_spread(Spread::Reflect);
    let expected_axis = g.axis();
    let atlas = SharedGradientAtlas::default();
    let row = atlas.register_stops(&g.stops, g.interp);
    let lowered = ResolvedGradient {
        axis: expected_axis,
        row,
        kind: FillKind::linear(g.spread),
    };
    let mut buffer = PaintCapture::default();
    buffer.draw_quad(DrawQuadPayload::rect(
        rect(0.0, 0.0, 100.0, 100.0),
        Corners::default(),
        BrushSource::Gradient(lowered),
        Stroke::ZERO.into(),
    ));
    let mut composer = composer();
    let mut out = render_buffer();
    composer
        .begin(
            params(1.0, UVec2::new(100, 100)),
            Duration::ZERO,
            &RecordPayloads::default(),
            &mut out,
        )
        .replay_from(&buffer);
    let q = &out.quads[0];
    assert_eq!(q.fill_kind, FillKind::linear(Spread::Reflect));
    assert!(q.fill_lut_row.0 >= 1, "linear quad must get a real row");
    assert_eq!(q.fill_axis, expected_axis);
}

/// Two quads referencing the same gradient share an atlas row.
/// Content-hash addressing keeps the bake step idempotent across
/// frames and across multiple emitting widgets.
#[test]
fn compose_repeated_linear_brush_shares_atlas_row() {
    use crate::primitives::brush::gradient::linear_geometry::LinearGradient;
    use crate::primitives::fill_kind::FillKind;
    use crate::renderer::gradient_atlas::shared_gradient_atlas::SharedGradientAtlas;
    let g = LinearGradient::two_stop(0.5, ColorU8::hex(0x336699), ColorU8::hex(0xddaa44));
    let atlas = SharedGradientAtlas::default();
    let lowered = ResolvedGradient {
        axis: g.axis(),
        row: atlas.register_stops(&g.stops, g.interp),
        kind: FillKind::linear(g.spread),
    };
    let mut buffer = PaintCapture::default();
    for _ in 0..3 {
        buffer.draw_quad(DrawQuadPayload::rect(
            rect(0.0, 0.0, 10.0, 10.0),
            Corners::default(),
            BrushSource::Gradient(lowered),
            Stroke::ZERO.into(),
        ));
    }
    let mut composer = composer();
    let mut out = render_buffer();
    composer
        .begin(
            params(1.0, UVec2::new(100, 100)),
            Duration::ZERO,
            &RecordPayloads::default(),
            &mut out,
        )
        .replay_from(&buffer);
    let rows: Vec<_> = out.quads.iter().map(|q| q.fill_lut_row).collect();
    assert_eq!(rows.len(), 3);
    assert_eq!(rows[0], rows[1]);
    assert_eq!(rows[1], rows[2]);
    // Row 0 is `LutRow::FALLBACK`, the permanent magenta row, so a fresh
    // atlas hands the first real gradient row 1.
    assert_eq!(rows[0], LutRow(1));
}

#[test]
fn compose_emits_image_batch_for_drawimage() {
    let buf = run(
        |b, _arena| {
            b.draw_image(
                DrawImagePayload::image(
                    rect(10.0, 20.0, 30.0, 40.0),
                    glam::Vec2::ZERO,
                    glam::Vec2::ONE,
                    Color::WHITE.into(),
                    TextureId(0xc0ffee),
                    0,
                ),
                None,
            );
        },
        &params(2.0, UVec2::new(400, 400)),
    );
    assert_eq!(buf.images.len(), 1, "one image draw");
    assert_eq!(buf.images.len(), 1, "one image instance");
    assert_eq!(buf.batches(PaintTier::Image).len(), 1, "one image batch");
    assert_eq!(buf.batches(PaintTier::Image)[0].items, Span::new(0, 1));
    assert_eq!(buf.images.id()[0], TextureId(0xc0ffee));
    // Physical-px rect = logical * scale (no snap in `params`).
    assert_eq!(buf.images.instance()[0].rect, rect(20.0, 40.0, 60.0, 80.0));
    // Composer must forward the encoder's UV crop verbatim — a Zero
    // UV size means "sample one texel forever" and silently paints
    // every image as a uniform color (regression hunt: 2026-05).
    assert_eq!(buf.images.instance()[0].uv_min, glam::Vec2::ZERO);
    assert_eq!(buf.images.instance()[0].uv_size, glam::Vec2::ONE);
}

#[test]
fn compose_gpu_view_carries_nested_transform_and_dpr_to_raster_target() {
    #[derive(Debug)]
    struct Case {
        dpr: f32,
        expected_size: UVec2,
        expected_raster_scale: f32,
    }

    let cases = [
        Case {
            dpr: 1.0,
            expected_size: UVec2::new(60, 30),
            expected_raster_scale: 3.0,
        },
        Case {
            dpr: 2.0,
            expected_size: UVec2::new(120, 60),
            expected_raster_scale: 6.0,
        },
    ];

    for case in cases {
        let buf = run(
            |b, _arena| {
                b.push_transform(TranslateScale::from_scale(2.0));
                b.push_transform(TranslateScale::from_scale(1.5));
                b.draw_image(
                    gpu_view_payload(rect(0.0, 0.0, 20.0, 10.0), TextureId(0xc0ffee)),
                    Some(&gpu_paint()),
                );
                b.pop_transform();
                b.pop_transform();
            },
            &params(case.dpr, UVec2::new(512, 512)),
        );

        assert_eq!(buf.frame_targets.len(), 1, "{case:?}");
        let target = &buf.frame_targets[0];
        assert_eq!(target.used, case.expected_size, "{case:?}");
        assert_eq!(target.display_scale, case.dpr, "{case:?}");
        assert_eq!(target.raster_scale, case.expected_raster_scale, "{case:?}");
        assert_eq!(
            buf.images.instance()[0].rect.size,
            Size::new(case.expected_size.x as f32, case.expected_size.y as f32),
            "{case:?}"
        );
    }
}

/// A view reaching past the surface is allocated for what is on screen, and
/// composited over that much of itself.
///
/// Layout is allowed to hand back a rect larger than the window — the
/// contains-content rule has a node overflow its parent rather than clip its own
/// content — so this is a state the composer must expect rather than one that
/// says something upstream went wrong. Following the rect would allocate pixels
/// the window can never show and ask the app to draw them.
///
/// Both halves are asked. A target sized to the visible part and a composite
/// still stretched across the whole rect would sample the view squashed, which
/// is worse than the waste it set out to save.
#[test]
fn compose_gpu_view_sized_to_what_the_surface_can_show() {
    // Twice as wide as the 100px surface, and a third taller.
    let buf = run(
        |b, _arena| {
            b.draw_image(
                gpu_view_payload(rect(0.0, 0.0, 200.0, 120.0), TextureId(0xc0ffee)),
                Some(&gpu_paint()),
            );
        },
        &params(1.0, UVec2::new(100, 90)),
    );

    assert_eq!(buf.frame_targets.len(), 1);
    let target = &buf.frame_targets[0];
    assert_eq!(
        target.used,
        UVec2::new(100, 90),
        "allocated past the window"
    );
    // The whole view is still reported, because that is the shape it was laid
    // out at and a projection derived from the visible part alone would be a
    // different aspect.
    assert_eq!(target.full, UVec2::new(200, 120));
    assert_eq!(
        target.offset,
        UVec2::ZERO,
        "cut off the far side, not the near"
    );
    // And the composite covers exactly what the target holds.
    assert_eq!(
        buf.images.instance()[0].rect,
        rect(0.0, 0.0, 100.0, 90.0),
        "the visible target was stretched over the whole rect"
    );
}

/// A view a clip cuts on its near side reports where the target begins.
///
/// What a scroll does: the pane shows a window onto the view, and the part
/// above and left of it is as unshowable as the part past the surface. The
/// offset is the half that only this case pins — an overflowing view is cut off
/// its far side, so its offset stays zero and a sign error there would not show.
#[test]
fn compose_gpu_view_sized_to_what_a_clip_leaves() {
    let buf = run(
        |b, _arena| {
            b.clip(PushClipPayload::rect(rect(30.0, 20.0, 40.0, 25.0)));
            b.draw_image(
                gpu_view_payload(rect(10.0, 10.0, 100.0, 60.0), TextureId(0xc0ffee)),
                Some(&gpu_paint()),
            );
            b.pop_clip();
        },
        &params(1.0, UVec2::new(200, 200)),
    );

    assert_eq!(buf.frame_targets.len(), 1);
    let target = &buf.frame_targets[0];
    // The clip runs 30..70 across and 20..45 down; the view runs 10..110 and
    // 10..70. What survives is the clip itself, 20 in and 10 down of the view.
    assert_eq!(target.used, UVec2::new(40, 25));
    assert_eq!(target.full, UVec2::new(100, 60));
    assert_eq!(target.offset, UVec2::new(20, 10));
    assert_eq!(buf.images.instance()[0].rect, rect(30.0, 20.0, 40.0, 25.0));
}

/// A view nothing cuts is left exactly as it was.
///
/// The path almost every frame takes, and the one the change above must not
/// disturb: the target is the whole view, it begins at its own corner, and the
/// composite covers the rect.
#[test]
fn compose_gpu_view_whole_when_nothing_clips_it() {
    let buf = run(
        |b, _arena| {
            b.draw_image(
                gpu_view_payload(rect(10.0, 20.0, 80.0, 40.0), TextureId(0xc0ffee)),
                Some(&gpu_paint()),
            );
        },
        &params(1.0, UVec2::new(200, 200)),
    );

    let target = &buf.frame_targets[0];
    assert_eq!(target.used, UVec2::new(80, 40));
    assert_eq!(target.full, target.used, "a whole view is its own whole");
    assert_eq!(target.offset, UVec2::ZERO);
    assert_eq!(buf.images.instance()[0].rect, rect(10.0, 20.0, 80.0, 40.0));
}

#[test]
fn compose_gpu_view_caps_wide_and_tall_targets_uniformly() {
    #[derive(Debug)]
    struct Case {
        logical_size: Size,
        expected_target: UVec2,
    }

    let cases = [
        Case {
            logical_size: Size::new(200.0, 50.0),
            expected_target: UVec2::new(100, 25),
        },
        Case {
            logical_size: Size::new(50.0, 200.0),
            expected_target: UVec2::new(25, 100),
        },
    ];

    for case in cases {
        let buf = run_with_texture_cap(
            |b, _arena| {
                b.draw_image(
                    gpu_view_payload(
                        Rect {
                            min: Vec2::ZERO,
                            size: case.logical_size,
                        },
                        TextureId(0xc0ffee),
                    ),
                    Some(&gpu_paint()),
                );
            },
            &params(1.0, UVec2::new(400, 400)),
            100,
        );

        assert_eq!(buf.frame_targets.len(), 1, "{case:?}");
        let target = &buf.frame_targets[0];
        assert_eq!(target.used, case.expected_target, "{case:?}");
        assert_eq!(target.display_scale, 1.0, "{case:?}");
        assert_eq!(target.raster_scale, 0.5, "{case:?}");
        assert_eq!(
            buf.images.instance()[0].rect.size,
            case.logical_size,
            "the composite destination stays at monitor resolution: {case:?}"
        );
        assert_eq!(
            target.used.x as f32 * case.logical_size.h,
            target.used.y as f32 * case.logical_size.w,
            "the capped target preserves the composite aspect ratio: {case:?}"
        );
        assert_eq!(target.full, target.used, "nothing clipped this one");
        assert_eq!(target.offset, UVec2::ZERO, "{case:?}");
    }

    // Capped *and* clipped, which is where the window could come apart from the
    // view it is a window onto: at a downsample its origin rounds down and its
    // size rounds up, so the pair is not obviously still inside the rounded-up
    // whole. It always is — the two roundings cannot sum past it — and this is
    // where that is held to, since nothing clamps it.
    //
    // The clip is deliberately off a whole number of target pixels — 45 logical
    // is 22.5 at half — so both roundings actually happen rather than the case
    // passing on exact arithmetic.
    let buf = run_with_texture_cap(
        |b, _arena| {
            b.clip(PushClipPayload::rect(rect(45.0, 45.0, 155.0, 155.0)));
            b.draw_image(
                gpu_view_payload(rect(0.0, 0.0, 200.0, 200.0), TextureId(0xc0ffee)),
                Some(&gpu_paint()),
            );
            b.pop_clip();
        },
        &params(1.0, UVec2::new(400, 400)),
        100,
    );
    let target = &buf.frame_targets[0];
    assert_eq!(target.full, UVec2::new(100, 100), "the whole view, halved");
    assert!(
        target.used.cmple(target.full).all(),
        "a window larger than the view it is a window onto: {target:?}"
    );
    assert!(
        (target.offset + target.used).cmple(target.full).all(),
        "a window reaching past the view: {target:?}"
    );
}

#[test]
fn compose_image_forwards_uv_crop_for_cover_fit() {
    let buf = run(
        |b, _arena| {
            b.draw_image(
                DrawImagePayload::image(
                    rect(0.0, 0.0, 100.0, 100.0),
                    glam::Vec2::new(0.25, 0.0),
                    glam::Vec2::new(0.5, 1.0),
                    Color::WHITE.into(),
                    TextureId(1),
                    0,
                ),
                None,
            );
        },
        &params(1.0, UVec2::new(400, 400)),
    );
    assert_eq!(buf.images.instance()[0].uv_min, glam::Vec2::new(0.25, 0.0));
    assert_eq!(buf.images.instance()[0].uv_size, glam::Vec2::new(0.5, 1.0));
}

/// The composer forwards `flags` verbatim and keeps each draw's UV as-is
/// (a `GpuView` ships full UV from the encoder — see `gpu_view` tests).
#[test]
fn compose_forwards_flags_and_repeat_uv() {
    use crate::renderer::render_buffer::image::{
        IMG_FLAG_MAG_NEAREST, IMG_FLAG_MIN_NEAREST, IMG_FLAG_TILED,
    };
    let buf = run(
        |b, _arena| {
            // Plain draw: flags stay 0.
            b.draw_image(
                DrawImagePayload::image(
                    rect(0.0, 0.0, 50.0, 50.0),
                    glam::Vec2::ZERO,
                    glam::Vec2::ONE,
                    Color::WHITE.into(),
                    TextureId(1),
                    0,
                ),
                None,
            );
            // Tiled draw: UV size > 1 (3×2 repeats) + tiled bit.
            b.draw_image(
                DrawImagePayload::image(
                    rect(0.0, 0.0, 50.0, 50.0),
                    glam::Vec2::ZERO,
                    glam::Vec2::new(3.0, 2.0),
                    Color::WHITE.into(),
                    TextureId(2),
                    IMG_FLAG_TILED,
                ),
                None,
            );
            // The two nearest-filter bits ride through together.
            b.draw_image(
                DrawImagePayload::image(
                    rect(0.0, 0.0, 50.0, 50.0),
                    glam::Vec2::ZERO,
                    glam::Vec2::ONE,
                    Color::WHITE.into(),
                    TextureId(3),
                    IMG_FLAG_MIN_NEAREST | IMG_FLAG_MAG_NEAREST,
                ),
                None,
            );
        },
        &params(1.0, UVec2::new(400, 400)),
    );
    assert_eq!(buf.images.instance()[0].flags, 0);
    assert_eq!(buf.images.instance()[1].flags, IMG_FLAG_TILED);
    assert_eq!(buf.images.instance()[1].uv_size, glam::Vec2::new(3.0, 2.0));
    assert_eq!(
        buf.images.instance()[2].flags,
        IMG_FLAG_MIN_NEAREST | IMG_FLAG_MAG_NEAREST
    );
}

#[test]
fn compose_image_curve_record_order_and_same_tier_gate_group_split() {
    let buf = run(
        |b, _| {
            image(b, rect(10.0, 10.0, 30.0, 30.0));
            curve(b, rect(0.0, 0.0, 100.0, 100.0));
        },
        &params(1.0, UVec2::new(200, 200)),
    );
    assert_eq!(buf.groups.len(), 1, "image then curve: replay == record");
    assert_eq!(buf.batches(PaintTier::Image)[0].last_group, 0);
    assert_eq!(buf.batches(PaintTier::Curve)[0].last_group, 0);

    let buf = run(
        |b, _| {
            curve(b, rect(0.0, 0.0, 100.0, 100.0));
            image(b, rect(10.0, 10.0, 30.0, 30.0));
        },
        &params(1.0, UVec2::new(200, 200)),
    );
    assert_eq!(
        buf.groups.len(),
        2,
        "curve then image: replay inverts record",
    );
    assert_eq!(buf.batches(PaintTier::Curve)[0].last_group, 0);
    assert_eq!(buf.batches(PaintTier::Image)[0].last_group, 1);

    let buf = run(
        |b, _| {
            curve(b, rect(0.0, 50.0, 100.0, 0.0));
            curve(b, rect(0.0, 50.0, 100.0, 0.0));
        },
        &params(1.0, UVec2::new(200, 200)),
    );
    assert_eq!(buf.groups.len(), 1, "same-tier order is stable");
    assert_eq!(buf.curves.len(), 2);
    assert_eq!(buf.batches(PaintTier::Curve).len(), 1);
    assert_eq!(buf.batches(PaintTier::Curve)[0].last_group, 0);
}
