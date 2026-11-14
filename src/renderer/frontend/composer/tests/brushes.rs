//! Fills, images and raster targets: what each emits and what rides with it.

use crate::common::span::Span;
use crate::internals::compose_rig::ComposeRig;
use crate::internals::paint_capture::PaintCapture;
use crate::primitives::geometry::corners::Corners;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::geometry::size::Size;
use crate::primitives::geometry::translate_scale::TranslateScale;
use crate::primitives::identity::effect_id::EffectId;
use crate::primitives::identity::texture_id::TextureId;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::packed::fill_axis::FillAxis;
use crate::primitives::packed::fill_kind::FillKind;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::lut_row::LutRow;
use crate::renderer::frontend::composer::tests::quad_builder::QuadBuilder;
use crate::renderer::frontend::composer::tests::support::{
    gpu_view, gpu_view_payload, params, rows_of, run, run_with_texture_cap,
};
use crate::renderer::frontend::paint_sink::{PaintPayload, PaintSink};
use crate::renderer::frontend::payload::brush_source::BrushSource;
use crate::renderer::frontend::payload::draw_closed_payload::DrawClosedPayload;
use crate::renderer::frontend::payload::draw_closed_payload::closed_effect::ClosedEffect;
use crate::renderer::frontend::payload::draw_closed_payload::closed_image::{
    ClosedImage, GpuViewDraw,
};
use crate::renderer::frontend::payload::push_clip_payload::PushClipPayload;
use crate::renderer::frontend::payload::resolved_gradient::ResolvedGradient;
use crate::renderer::frontend::payload::spin::Spin;
use crate::renderer::render_buffer::draw_run::Lane;
use crate::renderer::render_buffer::effect_draw_row::EffectKey;
use crate::renderer::render_buffer::fill_form::FillForm;
use crate::renderer::render_buffer::image_flags::ImageFlags;
use crate::renderer::render_buffer::paint_tier::PaintTier;
use crate::renderer::render_buffer::quad_draw_row::QuadKey;
use crate::renderer::render_buffer::shape_form::ShapeForm;
use crate::renderer::render_buffer::template_cover::TemplateCover;
use crate::shape::geometry::box_geometry::PlacedBox;
use crate::shape::geometry::closed_geometry::PlacedClosed;
use glam::{UVec2, Vec2};
use std::f32::consts::FRAC_PI_2;

/// A solid `Brush::Solid` panel emits a QuadInstance with `fill_kind = FillKind::SOLID`, `fill_lut_row = 0` (no gradient) and the fill color passed through.
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
    let q = &rig.out.quads.instance()[0];
    assert_eq!(rig.out.quads.key(), [QuadKey::Fill(FillForm::BOX_SOLID)]);
    assert_eq!(
        q.fill_kind,
        FillKind::SOLID,
        "solid quad must carry kind=solid"
    );
    assert_eq!(
        q.fill_lut_row,
        LutRow::FALLBACK,
        "solid quad has no LUT row",
    );
    assert_eq!(q.fill_axis, FillAxis::ZERO, "solid quad axis is zeroed");
}

/// A resolved linear gradient packs row, axis and kind into the paint payload,
/// which the composer pipes to the QuadInstance.
#[test]
fn compose_linear_brush_emits_kind_one_with_table_row() {
    use crate::primitives::packed::fill_kind::FillKind;
    use crate::primitives::paint::brush::gradient::Gradient;
    use crate::primitives::paint::brush::gradient::Spread;
    use crate::renderer::row_table::RowTable;
    let g = Gradient::horizontal([RgbaF32::WHITE, RgbaF32::BLACK]).with_spread(Spread::Reflect);
    let expected_axis = g.axis();
    let mut table = RowTable::default();
    let row = table.register(&g.ramp);
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
    let q = &rig.out.quads.instance()[0];
    assert_eq!(q.fill_kind, FillKind::linear(Spread::Reflect));
    assert_eq!(
        q.fill_lut_row,
        LutRow(1),
        "row 0 is the fallback, so the first gradient bakes into row 1"
    );
    assert_eq!(q.fill_axis, expected_axis);
}

/// Two quads referencing the same gradient share a table row (content-hash
/// addressing).
#[test]
fn compose_repeated_linear_brush_shares_table_row() {
    use crate::primitives::packed::fill_kind::FillKind;
    use crate::primitives::paint::brush::gradient::Gradient;
    use crate::renderer::row_table::RowTable;
    let g = Gradient::linear(0.5, [RgbaF32::hex(0x336699), RgbaF32::hex(0xddaa44)]);
    let mut table = RowTable::default();
    let lowered = ResolvedGradient {
        axis: g.axis(),
        lut_row: table.register(&g.ramp),
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
    let rows: Vec<_> = rig
        .out
        .quads
        .instance()
        .iter()
        .map(|q| q.fill_lut_row)
        .collect();
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
            b.draw(
                DrawClosedPayload {
                    geometry: PlacedClosed::Box(PlacedBox {
                        rect: Rect::new(10.0, 20.0, 30.0, 40.0),
                        corners: Corners::ZERO,
                    }),
                    spin: None,
                    paint: ClosedImage {
                        rect: Rect::new(10.0, 20.0, 30.0, 40.0),
                        uv_min: Vec2::ZERO,
                        uv_size: Vec2::ONE,
                        tint: RgbaF32::WHITE.to_f16(),
                        handle: TextureId(0xc0ffee),
                        flags: ImageFlags::NONE,
                        view: None,
                    },
                },
                1.0,
            );
        },
        &params(2.0, UVec2::new(400, 400)),
    );
    assert_eq!(buf.images.key(), [ShapeForm::Rect], "one plain image draw");
    assert_eq!(
        rows_of(&buf, Lane::Tier(PaintTier::Image)),
        [Span::new(0, 1)],
        "one image run"
    );
    assert_eq!(buf.images.id()[0], TextureId(0xc0ffee));
    assert_eq!(
        buf.images.instance()[0].header.rect,
        Rect::new(20.0, 40.0, 60.0, 80.0)
    );
    // The composer must forward the encoder's UV crop verbatim: a zero UV size
    // samples one texel forever and paints every image a uniform color.
    assert_eq!(buf.images.instance()[0].uv_min, Vec2::ZERO);
    assert_eq!(buf.images.instance()[0].uv_size, Vec2::ONE);
}

#[test]
fn compose_gpu_view_carries_nested_transform_and_dpr_to_raster_target() {
    #[derive(Debug)]
    struct Case {
        dpr: f32,
        expected_size: UVec2,
        expected_raster_factor: f32,
    }

    let cases = [
        Case {
            dpr: 1.0,
            expected_size: UVec2::new(60, 30),
            expected_raster_factor: 3.0,
        },
        Case {
            dpr: 2.0,
            expected_size: UVec2::new(120, 60),
            expected_raster_factor: 6.0,
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
        assert_eq!(
            target.raster_factor, case.expected_raster_factor,
            "{case:?}"
        );
        assert_eq!(
            buf.images.instance()[0].header.rect.size,
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
    let instance = buf.images.instance()[0];
    assert_eq!(
        instance.header.rect,
        Rect::new(0.0, 0.0, 100.0, 90.0),
        "the visible target was stretched over the whole rect"
    );
    // Both axes clip: 100 buckets to 128; 90 to 128, held at the whole 120.
    assert_eq!(target.capacity(), UVec2::new(128, 120));
    assert_eq!(instance.uv_min, Vec2::ZERO);
    assert_eq!(
        instance.uv_size,
        Vec2::new(100.0 / 128.0, 90.0 / 120.0),
        "the composite reads the shown corner alone"
    );
    assert_eq!(instance.flags, ImageFlags::CLAMP_TO_UV);
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
    let instance = buf.images.instance()[0];
    assert_eq!(instance.header.rect, Rect::new(30.0, 20.0, 40.0, 25.0));
    // 40 buckets to 64 and 25 to 32.
    assert_eq!(target.capacity(), UVec2::new(64, 32));
    assert_eq!(instance.uv_size, Vec2::new(40.0 / 64.0, 25.0 / 32.0));
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
    let instance = buf.images.instance()[0];
    assert_eq!(instance.header.rect, Rect::new(10.0, 20.0, 80.0, 40.0));
    assert_eq!(
        target.capacity(),
        target.used,
        "nothing clipped, nothing bucketed"
    );
    assert_eq!(instance.uv_size, Vec2::ONE);
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
        assert_eq!(target.raster_factor, 0.5, "{case:?}");
        assert_eq!(
            buf.images.instance()[0].header.rect.size,
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
            b.draw(
                DrawClosedPayload {
                    geometry: PlacedClosed::Box(PlacedBox {
                        rect: Rect::new(0.0, 0.0, 100.0, 100.0),
                        corners: Corners::ZERO,
                    }),
                    spin: None,
                    paint: ClosedImage {
                        rect: Rect::new(0.0, 0.0, 100.0, 100.0),
                        uv_min: Vec2::new(0.25, 0.0),
                        uv_size: Vec2::new(0.5, 1.0),
                        tint: RgbaF32::WHITE.to_f16(),
                        handle: TextureId(1),
                        flags: ImageFlags::NONE,
                        view: None,
                    },
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
            b.draw(
                DrawClosedPayload {
                    geometry: PlacedClosed::Box(PlacedBox {
                        rect: Rect::new(0.0, 0.0, 50.0, 50.0),
                        corners: Corners::ZERO,
                    }),
                    spin: None,
                    paint: ClosedImage {
                        rect: Rect::new(0.0, 0.0, 50.0, 50.0),
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
            // Tiled draw: UV size > 1 (3×2 repeats) + tiled bit.
            b.draw(
                DrawClosedPayload {
                    geometry: PlacedClosed::Box(PlacedBox {
                        rect: Rect::new(0.0, 0.0, 50.0, 50.0),
                        corners: Corners::ZERO,
                    }),
                    spin: None,
                    paint: ClosedImage {
                        rect: Rect::new(0.0, 0.0, 50.0, 50.0),
                        uv_min: Vec2::ZERO,
                        uv_size: Vec2::new(3.0, 2.0),
                        tint: RgbaF32::WHITE.to_f16(),
                        handle: TextureId(2),
                        flags: ImageFlags::TILED,
                        view: None,
                    },
                },
                1.0,
            );
            // The two nearest-filter bits ride through together.
            b.draw(
                DrawClosedPayload {
                    geometry: PlacedClosed::Box(PlacedBox {
                        rect: Rect::new(0.0, 0.0, 50.0, 50.0),
                        corners: Corners::ZERO,
                    }),
                    spin: None,
                    paint: ClosedImage {
                        rect: Rect::new(0.0, 0.0, 50.0, 50.0),
                        uv_min: Vec2::ZERO,
                        uv_size: Vec2::ONE,
                        tint: RgbaF32::WHITE.to_f16(),
                        handle: TextureId(3),
                        flags: ImageFlags::MIN_NEAREST.union(ImageFlags::MAG_NEAREST),
                        view: None,
                    },
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

/// An image turned a quarter about the logical pivot (50, 50) at scale 1: its rect
/// (40, 40)..(60, 50) has its center (50, 45) turned to (55, 50), so the instance is (45, 45) 20×10
/// at the angle, with its uv unchanged. A turned `GpuView` draws its whole rect: its visible part
/// is no rect of its own.
#[test]
fn a_turned_image_moves_its_center_and_keeps_its_uv() {
    const EPS: f32 = 1e-4;
    let spin = Some(Spin {
        pivot: Vec2::new(50.0, 50.0),
        angle: FRAC_PI_2,
    });
    let buf = run(
        |b, _| {
            b.draw(
                DrawClosedPayload {
                    geometry: PlacedClosed::Box(PlacedBox {
                        rect: Rect::new(40.0, 40.0, 20.0, 10.0),
                        corners: Corners::ZERO,
                    }),
                    spin,
                    paint: ClosedImage {
                        rect: Rect::new(40.0, 40.0, 20.0, 10.0),
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
        &params(1.0, UVec2::new(200, 200)),
    );
    let instance = buf.images.instance()[0];
    assert!(
        instance
            .header
            .rect
            .min
            .abs_diff_eq(Vec2::new(45.0, 45.0), EPS),
        "{:?}",
        instance.header.rect
    );
    assert_eq!(instance.header.rect.size, Size::new(20.0, 10.0));
    assert_eq!(instance.header.rotation, FRAC_PI_2);
    assert_eq!((instance.uv_min, instance.uv_size), (Vec2::ZERO, Vec2::ONE));

    let half_off = Rect::new(-10.0, 40.0, 20.0, 10.0);
    let buf = run(
        |b, _| {
            b.draw(
                {
                    let mut view = gpu_view_payload(half_off, TextureId(2)).turned(spin);
                    view.paint.view = Some(GpuViewDraw {
                        owner: WidgetId::from_hash("view"),
                        epoch: 1,
                    });
                    view
                },
                1.0,
            );
        },
        &params(1.0, UVec2::new(200, 200)),
    );
    assert_eq!(
        buf.frame_targets[0].used,
        UVec2::new(20, 10),
        "the whole view"
    );
}

/// A halo at scale 2 lands as a rim's row, its shape its own and its reach in physical px, and is
/// culled by what it covers. Over `(5, 6, 10, 8)` reaching 3, the row draws the rim of the box at
/// `(10, 12, 20, 16)` out to 6. A box whose right edge lies 2 logical px left of the view, at
/// `(−12, 20, 10, 8)`, reaches 1 px into it with a 3 px halo and lands; with a 1 px one it covers
/// nothing on the view and is culled. A reach of 0.1, 0.2 physical, lands at the antialiasing
/// reach, 0.5, so the outer half of the shape's edge draws whole.
#[test]
fn a_halo_lands_as_a_rim_culled_by_its_reach() {
    let halo = |rect: Rect, outside: f32| DrawClosedPayload {
        geometry: PlacedClosed::Box(PlacedBox {
            rect,
            corners: Corners::ZERO,
        }),
        spin: None,
        paint: ClosedEffect {
            effect: EffectId(3),
            params: Span::new(0, 0),
            images: Span::new(0, 0),
            backdrop: None,
            stage: None,
            outside: Some(outside),
            opacity: 1.0,
            time: 0.0,
        },
    };
    let buf = run(
        |b, _arena| {
            b.draw(halo(Rect::new(5.0, 6.0, 10.0, 8.0), 3.0), 1.0);
            b.draw(halo(Rect::new(-12.0, 20.0, 10.0, 8.0), 3.0), 1.0);
            b.draw(halo(Rect::new(-12.0, 20.0, 10.0, 8.0), 1.0), 1.0);
            b.draw(halo(Rect::new(40.0, 6.0, 10.0, 8.0), 0.1), 1.0);
        },
        &params(2.0, UVec2::new(400, 400)),
    );
    let rim = EffectKey {
        effect: EffectId(3),
        form: ShapeForm::Box,
        cover: TemplateCover::Rim,
    };
    assert_eq!(buf.effects.key(), [rim; 3]);
    let rows: Vec<(Rect, f32)> = buf
        .effects
        .instance()
        .iter()
        .map(|instance| (instance.header.rect, instance.reach))
        .collect();
    assert_eq!(
        rows,
        [
            (Rect::new(10.0, 12.0, 20.0, 16.0), 6.0),
            (Rect::new(-24.0, 40.0, 20.0, 16.0), 6.0),
            (Rect::new(80.0, 12.0, 20.0, 16.0), 0.5),
        ]
    );
}

/// An effect at scale 2 lands as one row: its rect in physical px, `(5, 6, 10, 8)` doubled to
/// `(10, 12, 20, 16)`; its corners `(4, 6, 6, 4)` doubled to `(8, 12, 12, 8)`, then fitted as CSS
/// fits them: the right side's two 12s sum 24 against its 16 px, so every radius scales by
/// `16 / 24`, to `(5.33, 8, 8, 5.33)`, 5.33 as the f16 lane holds it, 5.3320313; its parameters'
/// first word; its images' span; the fade a paint animation gave it, 0.5; the scale; and its time,
/// 1.25 s.
#[test]
fn an_effect_lands_as_one_physical_row() {
    let buf = run(
        |b, _arena| {
            b.draw(
                DrawClosedPayload {
                    geometry: PlacedClosed::Box(PlacedBox {
                        rect: Rect::new(5.0, 6.0, 10.0, 8.0),
                        corners: Corners::new(4.0, 6.0, 6.0, 4.0),
                    }),
                    spin: None,
                    paint: ClosedEffect {
                        effect: EffectId(3),
                        params: Span::new(12, 8),
                        images: Span::new(4, 2),
                        backdrop: None,
                        stage: None,
                        outside: None,
                        opacity: 1.0,
                        time: 1.25,
                    },
                },
                0.5,
            );
        },
        &params(2.0, UVec2::new(400, 400)),
    );
    assert_eq!(buf.effects.len(), 1);
    assert_eq!(
        rows_of(&buf, Lane::Tier(PaintTier::Effect)),
        [Span::new(0, 1)]
    );
    assert_eq!(
        (buf.effects.key()[0], buf.effects.instance()[0].flags),
        (
            EffectKey {
                effect: EffectId(3),
                form: ShapeForm::Box,
                cover: TemplateCover::Whole,
            },
            0
        )
    );
    assert_eq!(buf.effects.images()[0], Span::new(4, 2));
    let instance = buf.effects.instance()[0];
    assert_eq!(instance.header.rect, Rect::new(10.0, 12.0, 20.0, 16.0));
    assert_eq!(
        instance.header.corners.to_array(),
        [5.332_031_3, 8.0, 8.0, 5.332_031_3]
    );
    assert_eq!(instance.params, 12);
    assert_eq!(instance.opacity, 0.5);
    assert_eq!(instance.scale, 2.0);
    assert_eq!(instance.time, 1.25);
}
