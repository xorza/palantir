//! Fills, images and raster targets: what each emits and what rides with it.

use crate::common::span::Span;
use crate::internals::paint_capture::PaintCapture;
use crate::primitives::geometry::corners::Corners;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::geometry::size::Size;
use crate::primitives::geometry::translate_scale::TranslateScale;
use crate::primitives::identity::texture_id::TextureId;
use crate::primitives::packed::fill_axis::FillAxis;
use crate::primitives::packed::fill_kind::FillKind;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::lut_row::LutRow;
use crate::primitives::paint::stroke::Stroke;
use crate::renderer::frontend::composer::tests::compose_rig::ComposeRig;
use crate::renderer::frontend::composer::tests::quad_builder::QuadBuilder;
use crate::renderer::frontend::composer::tests::support::{
    curve, draw, gpu_view, image, params, run, run_with_texture_cap,
};
use crate::renderer::frontend::paint_sink::PaintSink;
use crate::renderer::frontend::payload::brush_source::BrushSource;
use crate::renderer::frontend::payload::draw_image_payload::{DrawImagePayload, ImageDraw};
use crate::renderer::frontend::payload::draw_quad_payload::DrawQuadPayload;
use crate::renderer::frontend::payload::push_clip_payload::PushClipPayload;
use crate::renderer::frontend::payload::resolved_gradient::ResolvedGradient;
use crate::renderer::render_buffer::image_flags::ImageFlags;
use crate::renderer::render_buffer::paint_tier::PaintTier;
use crate::shape::rect::RectKind;
use glam::{UVec2, Vec2};

/// A solid `Brush::Solid` panel emits a Quad with `fill_kind = BRUSH_KIND_SOLID
/// = 0`, `fill_lut_row = 0` (no gradient) and the fill colour passed through.
#[test]
fn compose_solid_brush_emits_kind_zero_quad() {
    let mut buffer = PaintCapture::default();
    QuadBuilder::new(Rect::new(0.0, 0.0, 100.0, 100.0))
        .solid(RgbaF32::srgb(0.5, 0.5, 0.5))
        .draw(&mut buffer);
    let mut rig = ComposeRig::new(params(1.0, UVec2::new(200, 200)));
    // 200x200 viewport: an opaque solid sharp quad covering it all would fold into
    // the clear instead of emitting.
    rig.compose(&buffer);
    let q = &rig.out.quads[0];
    assert_eq!(
        q.fill_kind,
        // Sharp, stroke-less, pixel-aligned: the solid kind also carries the
        // fragment fast-path bit.
        FillKind::SOLID.with_fast(),
        "solid quad must carry kind=solid (+fast)",
    );
    assert_eq!(
        q.fill_lut_row,
        LutRow::FALLBACK,
        "solid quad has no LUT row",
    );
    assert_eq!(q.fill_axis, FillAxis::ZERO, "solid quad axis is zeroed");
}

/// A windowed rect must not fold into the pass clear, take the fragment fast
/// path, or occlude quads beneath it (its interior is a hole). All three
/// compare `fill_kind == FillKind::SOLID`; the window bit breaks that by
/// design. Worst case: full-viewport, opaque, solid, sharp, pixel-aligned at
/// scale 1.
#[test]
fn windowed_rect_is_not_an_opaque_cover() {
    use crate::primitives::packed::fill_kind::FillKind;
    let buf = run(
        |b, _| {
            draw(b, Rect::new(10.0, 10.0, 50.0, 50.0));
            b.draw_quad(
                DrawQuadPayload::rect_of_kind(
                    RectKind::Windowed,
                    Rect::new(0.0, 0.0, 200.0, 200.0),
                    Corners::default(),
                    BrushSource::Solid(RgbaF32::srgb(1.0, 1.0, 1.0).into()),
                    Stroke::NONE.into(),
                ),
                1.0,
            );
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

/// A resolved linear gradient packs row, axis and kind into the paint payload,
/// which the composer pipes to the Quad.
#[test]
fn compose_linear_brush_emits_kind_one_with_atlas_row() {
    use crate::primitives::packed::fill_kind::FillKind;
    use crate::primitives::paint::brush::gradient::Spread;
    use crate::primitives::paint::brush::gradient::linear_geometry::LinearGradient;
    use crate::renderer::gradient_atlas::shared_gradient_atlas::SharedGradientAtlas;
    let g =
        LinearGradient::two_stop(0.0, RgbaF32::WHITE, RgbaF32::BLACK).with_spread(Spread::Reflect);
    let expected_axis = g.axis();
    let atlas = SharedGradientAtlas::default();
    let row = atlas.register(&g.ramp);
    let lowered = ResolvedGradient {
        axis: expected_axis,
        lut_row: row,
        kind: FillKind::linear(g.spread),
    };
    let mut buffer = PaintCapture::default();
    QuadBuilder::new(Rect::new(0.0, 0.0, 100.0, 100.0))
        .brush(BrushSource::Gradient(lowered))
        .draw(&mut buffer);
    let mut rig = ComposeRig::new(params(1.0, UVec2::new(100, 100)));
    rig.compose(&buffer);
    let q = &rig.out.quads[0];
    assert_eq!(q.fill_kind, FillKind::linear(Spread::Reflect));
    assert_eq!(
        q.fill_lut_row,
        LutRow(1),
        "row 0 is the fallback, so the first gradient bakes into row 1"
    );
    assert_eq!(q.fill_axis, expected_axis);
}

/// Two quads referencing the same gradient share an atlas row (content-hash
/// addressing).
#[test]
fn compose_repeated_linear_brush_shares_atlas_row() {
    use crate::primitives::packed::fill_kind::FillKind;
    use crate::primitives::paint::brush::gradient::linear_geometry::LinearGradient;
    use crate::renderer::gradient_atlas::shared_gradient_atlas::SharedGradientAtlas;
    let g = LinearGradient::two_stop(0.5, RgbaF32::hex(0x336699), RgbaF32::hex(0xddaa44));
    let atlas = SharedGradientAtlas::default();
    let lowered = ResolvedGradient {
        axis: g.axis(),
        lut_row: atlas.register(&g.ramp),
        kind: FillKind::linear(g.spread),
    };
    let mut buffer = PaintCapture::default();
    for _ in 0..3 {
        QuadBuilder::new(Rect::new(0.0, 0.0, 10.0, 10.0))
            .brush(BrushSource::Gradient(lowered))
            .draw(&mut buffer);
    }
    let mut rig = ComposeRig::new(params(1.0, UVec2::new(100, 100)));
    rig.compose(&buffer);
    let rows: Vec<_> = rig.out.quads.iter().map(|q| q.fill_lut_row).collect();
    assert_eq!(rows.len(), 3);
    assert_eq!(rows[0], rows[1]);
    assert_eq!(rows[1], rows[2]);
    // Row 0 is `LutRow::FALLBACK`, the magenta row, so the first real gradient
    // gets row 1.
    assert_eq!(rows[0], LutRow(1));
}

#[test]
fn compose_emits_image_batch_for_drawimage() {
    let buf = run(
        |b, _arena| {
            b.draw_image(
                ImageDraw {
                    payload: DrawImagePayload {
                        rect: Rect::new(10.0, 20.0, 30.0, 40.0),
                        uv_min: Vec2::ZERO,
                        uv_size: Vec2::ONE,
                        tint: RgbaF32::WHITE.into(),
                        handle: TextureId(0xc0ffee),
                        flags: ImageFlags::NONE,
                    },
                    view: None,
                },
                1.0,
            );
        },
        &params(2.0, UVec2::new(400, 400)),
    );
    assert_eq!(buf.images.len(), 1, "one image draw");
    assert_eq!(buf.images.len(), 1, "one image instance");
    assert_eq!(buf.batches(PaintTier::Image).len(), 1, "one image batch");
    assert_eq!(buf.batches(PaintTier::Image)[0].items, Span::new(0, 1));
    assert_eq!(buf.images.id()[0], TextureId(0xc0ffee));
    assert_eq!(
        buf.images.instance()[0].rect,
        Rect::new(20.0, 40.0, 60.0, 80.0)
    );
    // The composer must forward the encoder's UV crop verbatim: a zero UV size
    // samples one texel forever and paints every image a uniform colour.
    assert_eq!(buf.images.instance()[0].uv_min, Vec2::ZERO);
    assert_eq!(buf.images.instance()[0].uv_size, Vec2::ONE);
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
                gpu_view(b, Rect::new(0.0, 0.0, 20.0, 10.0), TextureId(0xc0ffee));
                b.pop_transform();
                b.pop_transform();
            },
            &params(case.dpr, UVec2::new(512, 512)),
        );

        assert_eq!(buf.frame_targets.len(), 1, "{case:?}");
        let target = &buf.frame_targets[0];
        assert_eq!(target.used, case.expected_size, "{case:?}");
        assert_eq!(buf.frame_views().display_scale, case.dpr, "{case:?}");
        assert_eq!(target.raster_scale, case.expected_raster_scale, "{case:?}");
        assert_eq!(
            buf.images.instance()[0].rect.size,
            Size::new(case.expected_size.x as f32, case.expected_size.y as f32),
            "{case:?}"
        );
    }
}

/// A view reaching past the surface is allocated for what is on screen and
/// composited over that much of itself.
///
/// Layout may hand back a rect larger than the window (overflow, not clip), so
/// the composer must expect it; following the rect would allocate pixels the
/// window can never show.
///
/// Both halves are asked: a target sized to the visible part with a composite
/// still stretched across the whole rect would squash the view.
#[test]
fn compose_gpu_view_sized_to_what_the_surface_can_show() {
    let buf = run(
        |b, _arena| {
            gpu_view(b, Rect::new(0.0, 0.0, 200.0, 120.0), TextureId(0xc0ffee));
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
    // The whole view is still reported: a projection from the visible part alone
    // would have a different aspect.
    assert_eq!(target.full, UVec2::new(200, 120));
    assert_eq!(
        target.offset,
        UVec2::ZERO,
        "cut off the far side, not the near"
    );
    // And the composite covers exactly what the target holds.
    assert_eq!(
        buf.images.instance()[0].rect,
        Rect::new(0.0, 0.0, 100.0, 90.0),
        "the visible target was stretched over the whole rect"
    );
}

/// A view a clip cuts on its near side reports where the target begins, as in
/// a scroll. The offset is the half only this case pins: an overflowing view is
/// cut on its far side, so its offset stays zero and a sign error wouldn't show.
#[test]
fn compose_gpu_view_sized_to_what_a_clip_leaves() {
    let buf = run(
        |b, _arena| {
            b.push_clip(PushClipPayload::rect(Rect::new(30.0, 20.0, 40.0, 25.0)));
            gpu_view(b, Rect::new(10.0, 10.0, 100.0, 60.0), TextureId(0xc0ffee));
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
    assert_eq!(
        buf.images.instance()[0].rect,
        Rect::new(30.0, 20.0, 40.0, 25.0)
    );
}

/// A view nothing cuts is left as it was: the target is the whole view, begins
/// at its own corner, and the composite covers the rect.
#[test]
fn compose_gpu_view_whole_when_nothing_clips_it() {
    let buf = run(
        |b, _arena| {
            gpu_view(b, Rect::new(10.0, 20.0, 80.0, 40.0), TextureId(0xc0ffee));
        },
        &params(1.0, UVec2::new(200, 200)),
    );

    let target = &buf.frame_targets[0];
    assert_eq!(target.used, UVec2::new(80, 40));
    assert_eq!(target.full, target.used, "a whole view is its own whole");
    assert_eq!(target.offset, UVec2::ZERO);
    assert_eq!(
        buf.images.instance()[0].rect,
        Rect::new(10.0, 20.0, 80.0, 40.0)
    );
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
                gpu_view(
                    b,
                    Rect {
                        min: Vec2::ZERO,
                        size: case.logical_size,
                    },
                    TextureId(0xc0ffee),
                );
            },
            &params(1.0, UVec2::new(400, 400)),
            100,
        );

        assert_eq!(buf.frame_targets.len(), 1, "{case:?}");
        let target = &buf.frame_targets[0];
        assert_eq!(target.used, case.expected_target, "{case:?}");
        assert_eq!(buf.frame_views().display_scale, 1.0, "{case:?}");
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

    // Capped *and* clipped: at a downsample the window's origin rounds down and
    // size rounds up, so the pair might exceed the rounded-up whole. The two
    // roundings cannot sum past it, and nothing clamps it, so this holds it.
    // The clip is off a whole target pixel (45 logical is 22.5 at half) so both
    // roundings happen.
    let buf = run_with_texture_cap(
        |b, _arena| {
            b.push_clip(PushClipPayload::rect(Rect::new(45.0, 45.0, 155.0, 155.0)));
            gpu_view(b, Rect::new(0.0, 0.0, 200.0, 200.0), TextureId(0xc0ffee));
            b.pop_clip();
        },
        &params(1.0, UVec2::new(400, 400)),
        100,
    );
    let target = &buf.frame_targets[0];
    assert_eq!(target.full, UVec2::new(100, 100), "the whole view, halved");
    // The clip leaves 45..200; at the cap's 0.5 that is 22.5..100, so the window
    // starts on the floor, 22, and ends at the view's edge, 100.
    assert_eq!(target.offset, UVec2::splat(22));
    assert_eq!(target.used, UVec2::splat(100 - 22));
}

#[test]
fn compose_image_forwards_uv_crop_for_cover_fit() {
    let buf = run(
        |b, _arena| {
            b.draw_image(
                ImageDraw {
                    payload: DrawImagePayload {
                        rect: Rect::new(0.0, 0.0, 100.0, 100.0),
                        uv_min: Vec2::new(0.25, 0.0),
                        uv_size: Vec2::new(0.5, 1.0),
                        tint: RgbaF32::WHITE.into(),
                        handle: TextureId(1),
                        flags: ImageFlags::NONE,
                    },
                    view: None,
                },
                1.0,
            );
        },
        &params(1.0, UVec2::new(400, 400)),
    );
    assert_eq!(buf.images.instance()[0].uv_min, Vec2::new(0.25, 0.0));
    assert_eq!(buf.images.instance()[0].uv_size, Vec2::new(0.5, 1.0));
}

/// The composer forwards `flags` verbatim and keeps each draw's UV as-is.
#[test]
fn compose_forwards_flags_and_repeat_uv() {
    let buf = run(
        |b, _arena| {
            // Plain draw: no flags.
            b.draw_image(
                ImageDraw {
                    payload: DrawImagePayload {
                        rect: Rect::new(0.0, 0.0, 50.0, 50.0),
                        uv_min: Vec2::ZERO,
                        uv_size: Vec2::ONE,
                        tint: RgbaF32::WHITE.into(),
                        handle: TextureId(1),
                        flags: ImageFlags::NONE,
                    },
                    view: None,
                },
                1.0,
            );
            // Tiled draw: UV size > 1 (3×2 repeats) + tiled bit.
            b.draw_image(
                ImageDraw {
                    payload: DrawImagePayload {
                        rect: Rect::new(0.0, 0.0, 50.0, 50.0),
                        uv_min: Vec2::ZERO,
                        uv_size: Vec2::new(3.0, 2.0),
                        tint: RgbaF32::WHITE.into(),
                        handle: TextureId(2),
                        flags: ImageFlags::TILED,
                    },
                    view: None,
                },
                1.0,
            );
            // The two nearest-filter bits ride through together.
            b.draw_image(
                ImageDraw {
                    payload: DrawImagePayload {
                        rect: Rect::new(0.0, 0.0, 50.0, 50.0),
                        uv_min: Vec2::ZERO,
                        uv_size: Vec2::ONE,
                        tint: RgbaF32::WHITE.into(),
                        handle: TextureId(3),
                        flags: ImageFlags::MIN_NEAREST.union(ImageFlags::MAG_NEAREST),
                    },
                    view: None,
                },
                1.0,
            );
        },
        &params(1.0, UVec2::new(400, 400)),
    );
    assert_eq!(buf.images.instance()[0].flags, ImageFlags::NONE);
    assert_eq!(buf.images.instance()[1].flags, ImageFlags::TILED);
    assert_eq!(buf.images.instance()[1].uv_size, Vec2::new(3.0, 2.0));
    assert_eq!(
        buf.images.instance()[2].flags,
        ImageFlags::MIN_NEAREST.union(ImageFlags::MAG_NEAREST)
    );
}

#[test]
fn compose_image_curve_record_order_and_same_tier_gate_group_split() {
    let buf = run(
        |b, _| {
            image(b, Rect::new(10.0, 10.0, 30.0, 30.0));
            curve(b, Rect::new(0.0, 0.0, 100.0, 100.0));
        },
        &params(1.0, UVec2::new(200, 200)),
    );
    assert_eq!(buf.groups.len(), 1, "image then curve: replay == record");
    assert_eq!(buf.batches(PaintTier::Image)[0].last_group, 0);
    assert_eq!(buf.batches(PaintTier::Curve)[0].last_group, 0);

    let buf = run(
        |b, _| {
            curve(b, Rect::new(0.0, 0.0, 100.0, 100.0));
            image(b, Rect::new(10.0, 10.0, 30.0, 30.0));
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
            curve(b, Rect::new(0.0, 50.0, 100.0, 0.0));
            curve(b, Rect::new(0.0, 50.0, 100.0, 0.0));
        },
        &params(1.0, UVec2::new(200, 200)),
    );
    assert_eq!(buf.groups.len(), 1, "same-tier order is stable");
    assert_eq!(buf.curves.len(), 2);
    assert_eq!(buf.batches(PaintTier::Curve).len(), 1);
    assert_eq!(buf.batches(PaintTier::Curve)[0].last_group, 0);
}
