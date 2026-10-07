//! Which draws share a group and a batch, and what forces a split.

use crate::common::span::Span;
use crate::icons::icon_set::IconRef;
use crate::internals::paint_capture::PaintCapture;
use crate::primitives::geometry::corners::Corners;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::geometry::spacing::Spacing;
use crate::primitives::geometry::urect::URect;
use crate::primitives::identity::texture_id::TextureId;
use crate::primitives::packed::fill_axis::FillAxis;
use crate::primitives::packed::fill_kind::FillKind;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::stroke::Stroke;
use crate::renderer::frontend::composer::tests::compose_rig::ComposeRig;
use crate::renderer::frontend::composer::tests::quad_builder::QuadBuilder;
use crate::renderer::frontend::composer::tests::support::{
    clip, clip_rounded, curve, draw, gpu_view_payload, icon, image, inked_text, mesh, params,
    params_unsnapped, polyline_cmd, run, text,
};
use crate::renderer::frontend::paint_sink::PaintSink;
use crate::renderer::frontend::payload::brush_source::BrushSource;
use crate::renderer::frontend::payload::draw_image_payload::ImageDraw;
use crate::renderer::frontend::payload::draw_polyline_payload::DrawPolylinePayload;
use crate::renderer::frontend::payload::draw_quad_payload::DrawQuadPayload;
use crate::renderer::frontend::payload::resolved_gradient::ResolvedGradient;
use crate::renderer::frontend::payload::stroke_bounds::Spin;
use crate::renderer::frontend::payload::stroke_bounds::StrokeBounds;
use crate::renderer::render_buffer::paint_tier::PaintTier;
use crate::scene::record_store::RecordStore;
use crate::shape::record::ColorMode;
use crate::shape::style::{LineCap, LineJoin};
use glam::{UVec2, Vec2};
use std::f32::consts::FRAC_PI_2;

/// `Quad -> Text -> Quad` in one scissor makes two groups, so the second quad renders after the text.
#[test]
fn compose_splits_group_on_text_to_quad_transition() {
    let buf = run(
        |b, _arena| {
            draw(b, Rect::new(0.0, 0.0, 100.0, 100.0));
            text(b, Rect::new(10.0, 10.0, 80.0, 20.0));
            draw(b, Rect::new(20.0, 20.0, 60.0, 40.0));
        },
        &params(1.0, UVec2::new(200, 200)),
    );
    assert_eq!(buf.quads.len(), 2);
    assert_eq!(buf.texts.len(), 1);
    assert_eq!(
        buf.groups.len(),
        2,
        "text→quad transition must start a new group"
    );
    assert_eq!(buf.groups[0].quads, Span::new(0, 1));
    assert_eq!(buf.text_batches.len(), 1);
    assert_eq!(buf.text_batches[0].texts, Span::new(0, 1));
    assert_eq!(buf.text_batches[0].last_group, 0);
    assert_eq!(buf.groups[1].quads, Span::new(1, 1));
}

#[test]
fn compose_does_not_split_consecutive_texts() {
    let buf = run(
        |b, _arena| {
            draw(b, Rect::new(0.0, 0.0, 100.0, 100.0));
            text(b, Rect::new(10.0, 10.0, 80.0, 20.0));
            text(b, Rect::new(10.0, 35.0, 80.0, 20.0));
        },
        &params(1.0, UVec2::new(200, 200)),
    );
    assert_eq!(buf.quads.len(), 1);
    assert_eq!(buf.texts.len(), 2);
    assert_eq!(buf.groups.len(), 1);
    assert_eq!(buf.groups[0].quads, Span::new(0, 1));
    assert_eq!(buf.text_batches.len(), 1);
    assert_eq!(buf.text_batches[0].texts, Span::new(0, 2));
    assert_eq!(buf.text_batches[0].last_group, 0);
}

/// A redundant nested clip (same scissor as its parent) keeps overlap state across the push/pop.
#[test]
fn compose_same_clip_push_pop_preserves_overlap_state() {
    let buf = run(
        |b, _arena| {
            clip(b, Rect::new(0.0, 0.0, 200.0, 200.0));
            draw(b, Rect::new(0.0, 0.0, 100.0, 28.0)); // node A bg
            text(b, Rect::new(4.0, 4.0, 90.0, 20.0)); //  node A label
            clip(b, Rect::new(0.0, 0.0, 200.0, 200.0));
            b.pop_clip();
            draw(b, Rect::new(40.0, 10.0, 100.0, 28.0)); // node B bg, overlaps A's label
            b.pop_clip();
        },
        &params(1.0, UVec2::new(400, 400)),
    );
    assert_eq!(buf.quads.len(), 2);
    assert_eq!(buf.texts.len(), 1);
    assert_eq!(
        buf.groups.len(),
        2,
        "overlap state must survive a redundant clip Push/Pop",
    );
}

/// Non-overlapping `(quad, text)` rows batch into one group; a quad flushes only when it intersects a prior text.
#[test]
fn compose_batches_disjoint_row_units_into_one_group() {
    let buf = run(
        |b, _arena| {
            for i in 0..5 {
                let y = (i as f32) * 40.0;
                draw(b, Rect::new(0.0, y, 100.0, 28.0));
                text(b, Rect::new(4.0, y + 4.0, 90.0, 20.0));
            }
        },
        &params(1.0, UVec2::new(200, 400)),
    );
    assert_eq!(buf.quads.len(), 5);
    assert_eq!(buf.texts.len(), 5);
    assert_eq!(
        buf.groups.len(),
        1,
        "disjoint (quad,text) rows must batch into one group",
    );
    assert_eq!(buf.groups[0].quads, Span::new(0, 5));
    assert_eq!(buf.text_batches.len(), 1, "one texts batch for all rows");
    assert_eq!(buf.text_batches[0].texts, Span::new(0, 5));
}

/// A later quad overlapping a prior text must flush to preserve paint order.
#[test]
fn compose_flushes_when_later_quad_overlaps_prior_text() {
    let buf = run(
        |b, _arena| {
            draw(b, Rect::new(0.0, 0.0, 100.0, 28.0)); // node A chrome
            text(b, Rect::new(4.0, 4.0, 90.0, 20.0)); //  node A label
            draw(b, Rect::new(40.0, 10.0, 100.0, 28.0)); // node B chrome, overlaps A's label
            text(b, Rect::new(44.0, 14.0, 90.0, 20.0)); // node B label
        },
        &params(1.0, UVec2::new(400, 200)),
    );
    assert_eq!(buf.quads.len(), 2);
    assert_eq!(buf.texts.len(), 2);
    assert_eq!(
        buf.groups.len(),
        2,
        "overlapping quad-after-text must start a new group",
    );
}

#[test]
fn compose_shadow_outer_halo_after_text_splits_group() {
    let sigma = 4.0;
    // The composer grows the source by 4σ = 16, to x = 34: past the text's 39.
    let source = Rect::new(50.0, 50.0, 50.0, 50.0);
    let buf = run(
        |b, _arena| {
            text(b, Rect::new(39.0, 60.0, 2.0, 10.0));
            b.draw_quad(
                DrawQuadPayload::shadow(
                    source,
                    Corners::ZERO,
                    RgbaF32::BLACK.into(),
                    FillKind::SHADOW_DROP,
                    FillAxis::from_lanes(0.0, 0.0, sigma, 0.0),
                ),
                1.0,
            );
        },
        &params(1.0, UVec2::new(200, 200)),
    );

    assert_eq!(buf.groups.len(), 2, "outer halo overlap must split");
    assert_eq!(buf.text_batches[0].last_group, 0);
    assert_eq!(buf.groups[1].quads, Span::new(0, 1));
}

#[test]
fn compose_keeps_quads_then_text_in_one_group() {
    let buf = run(
        |b, _arena| {
            draw(b, Rect::new(0.0, 0.0, 100.0, 100.0));
            draw(b, Rect::new(2.0, 2.0, 96.0, 96.0));
            text(b, Rect::new(10.0, 10.0, 80.0, 20.0));
        },
        &params(1.0, UVec2::new(200, 200)),
    );
    assert_eq!(buf.groups.len(), 1);
    assert_eq!(buf.groups[0].quads, Span::new(0, 2));
    assert_eq!(buf.text_batches.len(), 1);
    assert_eq!(buf.text_batches[0].texts, Span::new(0, 1));
    assert_eq!(buf.text_batches[0].last_group, 0);
}

/// Rows in separate scissors still coalesce their text into one batch.
#[test]
fn compose_coalesces_text_across_distinct_scissor_groups() {
    let buf = run(
        |b, _arena| {
            clip(b, Rect::new(0.0, 0.0, 100.0, 30.0));
            draw(b, Rect::new(0.0, 0.0, 100.0, 28.0));
            text(b, Rect::new(4.0, 4.0, 90.0, 20.0));
            b.pop_clip();
            clip(b, Rect::new(0.0, 40.0, 100.0, 30.0));
            draw(b, Rect::new(0.0, 40.0, 100.0, 28.0));
            text(b, Rect::new(4.0, 44.0, 90.0, 20.0));
            b.pop_clip();
        },
        &params(1.0, UVec2::new(200, 200)),
    );
    assert_eq!(buf.groups.len(), 2, "distinct scissors → distinct groups");
    assert_eq!(
        buf.text_batches.len(),
        1,
        "non-overlapping rows must share one text batch",
    );
    assert_eq!(buf.text_batches[0].texts.len, 2);
}

/// A clipped text run lands in a batch whose scissor equals its clipped bounds (the text shader has no per-instance clip); wider neighbour text forces a split.
#[test]
fn compose_clipped_text_overflow_does_not_widen_batch_scissor() {
    let buf = run(
        |b, _arena| {
            text(b, Rect::new(0.0, 0.0, 200.0, 20.0));
            clip(b, Rect::new(40.0, 40.0, 20.0, 20.0));
            text(b, Rect::new(40.0, 40.0, 100.0, 20.0));
            b.pop_clip();
        },
        &params(1.0, UVec2::new(300, 300)),
    );
    assert_eq!(
        buf.text_batches.len(),
        2,
        "strict (clipped-narrower) text must not coalesce with wider neighbours",
    );
    let strict = buf
        .text_batches
        .iter()
        .find(|tb| tb.scissor.size.x == 20)
        .expect("expected a batch with 20px-wide scissor");
    assert_eq!(strict.scissor.size.x, 20);
    assert_eq!(strict.scissor.size.y, 20);
}

#[test]
fn compose_strict_text_with_matching_clip_coalesces() {
    let clip_rect = Rect::new(40.0, 40.0, 20.0, 20.0);
    let buf = run(
        |b, _arena| {
            clip(b, clip_rect);
            text(b, Rect::new(40.0, 40.0, 100.0, 20.0));
            b.pop_clip();
            clip(b, clip_rect);
            text(b, Rect::new(40.0, 40.0, 100.0, 20.0));
            b.pop_clip();
        },
        &params(1.0, UVec2::new(300, 300)),
    );
    assert_eq!(
        buf.text_batches.len(),
        1,
        "two strict runs with identical clip bounds should share a batch",
    );
}

/// A rounded-clip change splits the text batch (different stencil masks); each batch carries its runs' mask chain so the schedule can stencil it past skipped groups.
#[test]
fn compose_rounded_clip_change_splits_text_batch() {
    let buf = run(
        |b, _arena| {
            clip_rounded(b, Rect::new(0.0, 0.0, 100.0, 30.0), Corners::all(4.0));
            text(b, Rect::new(4.0, 4.0, 90.0, 20.0));
            b.pop_clip();
            clip_rounded(b, Rect::new(0.0, 40.0, 100.0, 30.0), Corners::all(8.0));
            text(b, Rect::new(4.0, 44.0, 90.0, 20.0));
            b.pop_clip();
        },
        &params(1.0, UVec2::new(200, 200)),
    );
    assert_eq!(buf.text_batches.len(), 2, "rounded change must split batch");
    for (i, tb) in buf.text_batches.iter().enumerate() {
        let batch_chain = &buf.rounded_clips[tb.rounded_clips.range()];
        assert_eq!(batch_chain.len(), 1, "batch {i} recorded under one mask");
        let group_chain =
            &buf.rounded_clips[buf.groups[tb.last_group as usize].rounded_clips.range()];
        assert_eq!(
            batch_chain, group_chain,
            "batch {i} chain matches its last_group's chain"
        );
    }
    let r0 = buf.rounded_clips[buf.text_batches[0].rounded_clips.range()][0];
    let r1 = buf.rounded_clips[buf.text_batches[1].rounded_clips.range()][0];
    assert_eq!(r0.corners.as_array()[0], 4.0);
    assert_eq!(r1.corners.as_array()[0], 8.0);
}

/// The composer rotates each polyline point about `bbox.center()` before the ancestor transform: a 90 degree spin of a centred horizontal segment is vertical and centred.
#[test]
fn compose_spins_polyline_about_bbox_center() {
    let aabb = |rotation: f32| -> (Vec2, Vec2) {
        let mut buffer = PaintCapture::default();
        let mut store = RecordStore::default();
        let p_start = store.polyline_points.len() as u32;
        store.polyline_points.push(Vec2::new(15.0, 50.0));
        store.polyline_points.push(Vec2::new(85.0, 50.0));
        let c_start = store.polyline_colors.len() as u32;
        store.polyline_colors.push(RgbaF32::WHITE.into());
        buffer.draw_polyline(
            DrawPolylinePayload {
                alpha: 1.0,
                bounds: if rotation == 0.0 {
                    StrokeBounds::Still(Rect::new(0.0, 0.0, 100.0, 100.0))
                } else {
                    StrokeBounds::Spun {
                        spin: Spin {
                            pivot: Vec2::splat(50.0),
                            angle: rotation,
                        },
                        radius: Vec2::splat(50.0).length(),
                    }
                },
                origin: Vec2::ZERO,
                width: 2.0,
                points_start: p_start,
                points_len: 2,
                colors_start: c_start,
                colors_len: 1,
                color_mode: ColorMode::Single,
                cap: LineCap::Butt,
                join: LineJoin::Miter,
            },
            1.0,
        );
        let mut rig = ComposeRig::new(params(1.0, UVec2::new(200, 200)));
        rig.store = store;
        rig.compose(&buffer);
        assert_eq!(rig.out.curves.len(), 1, "one segment instance");
        let ci = &rig.out.curves[0];
        (ci.p0.min(ci.p3), ci.p0.max(ci.p3))
    };
    // Spun 90 degrees about (50, 50): ends at (50, 15) and (50, 85).
    assert_eq!(aabb(0.0), (Vec2::new(15.0, 50.0), Vec2::new(85.0, 50.0)));
    assert_eq!(
        aabb(FRAC_PI_2),
        (Vec2::new(50.0, 15.0), Vec2::new(50.0, 85.0))
    );
}

/// A culled higher-kind draw does not split the text batch: the cull runs before the overlap test.
#[test]
fn compose_culled_mesh_over_batch_text_keeps_one_batch() {
    let label = Rect::new(0.0, 0.0, 100.0, 20.0);
    let buf = run(
        |b, _arena| {
            clip(b, Rect::new(0.0, 0.0, 400.0, 400.0));
            text(b, label);
            b.pop_clip();
            clip(b, Rect::new(0.0, 200.0, 100.0, 100.0));
            mesh(b, label); // covers the label, but the clip discards it
            b.pop_clip();
            text(b, Rect::new(0.0, 40.0, 100.0, 20.0));
        },
        &params(1.0, UVec2::new(400, 400)),
    );
    assert_eq!(buf.meshes.len(), 0, "the mesh must be culled");
    assert_eq!(
        buf.text_batches.len(),
        1,
        "a culled mesh must not split the text batch",
    );
}

/// A quad overlapping prior batch text closes the batch: two groups, two text batches.
#[test]
fn compose_quad_overlap_with_prior_batch_text_splits_batch() {
    let buf = run(
        |b, _arena| {
            text(b, Rect::new(0.0, 0.0, 100.0, 30.0)); // text A
            clip(b, Rect::new(0.0, 0.0, 200.0, 200.0));
            draw(b, Rect::new(10.0, 10.0, 50.0, 20.0)); // overlaps A → must close batch
            b.pop_clip();
            text(b, Rect::new(0.0, 40.0, 100.0, 30.0)); // text B
        },
        &params(1.0, UVec2::new(400, 400)),
    );
    assert_eq!(
        buf.text_batches.len(),
        2,
        "quad overlapping prior batch text must split the batch",
    );
}

#[test]
fn tight_curve_bound_avoids_false_group_split() {
    #[derive(Debug)]
    struct Case {
        image_x: f32,
        expected_groups: usize,
    }

    // The curve ends at x=20; width 2 + 0.5 AA bounds it at ceil(21.5)=22. x=22 is disjoint; one pixel left overlaps.
    let cases = [
        Case {
            image_x: 22.0,
            expected_groups: 1,
        },
        Case {
            image_x: 21.0,
            expected_groups: 2,
        },
    ];

    for case in cases {
        let buf = run(
            |b, _| {
                curve(b, Rect::new(0.0, 0.0, 20.0, 20.0));
                image(b, Rect::new(case.image_x, 0.0, 10.0, 10.0));
            },
            &params(1.0, UVec2::new(100, 100)),
        );
        assert_eq!(buf.groups.len(), case.expected_groups, "{case:?}");
    }
}

#[test]
fn compose_mesh_then_overlapping_curve_keeps_one_group() {
    let buf = run(
        |b, _| {
            mesh(b, Rect::new(10.0, 10.0, 30.0, 30.0));
            curve(b, Rect::new(0.0, 0.0, 100.0, 100.0));
        },
        &params(1.0, UVec2::new(200, 200)),
    );
    assert_eq!(buf.groups.len(), 1, "record order matches replay order");
    assert_eq!(buf.batches(PaintTier::Mesh)[0].last_group, 0);
    assert_eq!(buf.batches(PaintTier::Curve)[0].last_group, 0);
}

/// Mesh then image replays in record order (one group); image then mesh inverts it and flushes.
#[test]
fn compose_mesh_image_record_order_gates_group_split() {
    let buf = run(
        |b, _| {
            mesh(b, Rect::new(10.0, 10.0, 30.0, 30.0));
            image(b, Rect::new(20.0, 20.0, 30.0, 30.0)); // overlaps the mesh
        },
        &params(1.0, UVec2::new(200, 200)),
    );
    assert_eq!(buf.groups.len(), 1, "mesh then image: replay == record");
    assert_eq!(buf.batches(PaintTier::Mesh)[0].last_group, 0);
    assert_eq!(buf.batches(PaintTier::Image)[0].last_group, 0);

    let buf = run(
        |b, _| {
            image(b, Rect::new(20.0, 20.0, 30.0, 30.0));
            mesh(b, Rect::new(10.0, 10.0, 30.0, 30.0)); // overlaps the image
        },
        &params(1.0, UVec2::new(200, 200)),
    );
    assert_eq!(
        buf.groups.len(),
        2,
        "image then mesh: replay inverts record",
    );
    assert_eq!(buf.batches(PaintTier::Image)[0].last_group, 0);
    assert_eq!(buf.batches(PaintTier::Mesh)[0].last_group, 1);
}

/// Non-overlapping mixed kinds share one group. Inflated bounds: curve (0,0)..(22,22), mesh (39,39)..(61,61), image (80,80,20,20) exact.
#[test]
fn compose_disjoint_mixed_kinds_share_one_group() {
    let buf = run(
        |b, _| {
            curve(b, Rect::new(0.0, 0.0, 20.0, 20.0));
            mesh(b, Rect::new(40.0, 40.0, 20.0, 20.0));
            image(b, Rect::new(80.0, 80.0, 20.0, 20.0));
        },
        &params(1.0, UVec2::new(200, 200)),
    );
    assert_eq!(buf.groups.len(), 1, "disjoint kinds must not split");
    assert_eq!(buf.batches(PaintTier::Curve)[0].last_group, 0);
    assert_eq!(buf.batches(PaintTier::Mesh)[0].last_group, 0);
    assert_eq!(buf.batches(PaintTier::Image)[0].last_group, 0);
}

// Pruning drops a quad iff a later quad in its group fully covers its painted extent (`q.rect`) under `Rect::contains_rect`.

/// A quad overlapping text in an already-closed batch of the same group must still flush.
#[test]
fn quad_flushes_text_in_already_closed_batch_same_group() {
    let buf = run(
        |b, store| {
            text(b, Rect::new(0.0, 0.0, 300.0, 20.0));
            polyline_cmd(
                b,
                store,
                &[Vec2::new(200.0, 10.0), Vec2::new(300.0, 10.0)],
                &[RgbaF32::WHITE],
                ColorMode::Single,
                1.0,
                LineCap::Butt,
                LineJoin::Miter,
            );
            draw(b, Rect::new(0.0, 0.0, 100.0, 60.0));
            text(b, Rect::new(0.0, 100.0, 300.0, 20.0));
            polyline_cmd(
                b,
                store,
                &[Vec2::new(200.0, 110.0), Vec2::new(300.0, 110.0)],
                &[RgbaF32::WHITE],
                ColorMode::Single,
                1.0,
                LineCap::Butt,
                LineJoin::Miter,
            );
            draw(b, Rect::new(0.0, 100.0, 100.0, 60.0));
        },
        &params(1.0, UVec2::new(600, 600)),
    );
    assert_eq!(buf.text_batches.len(), 2);
    // Each box is padded 0.25 % a side (0.75 px across, 0.05 down) and covered; the first clamps to (0, 0)..(301, 21), the second spans y 99.95..120.05, so 99..121.
    assert_eq!(buf.text_batches[0].scissor, URect::new(0, 0, 301, 21));
    assert_eq!(buf.text_batches[1].scissor, URect::new(0, 99, 301, 22));
    for (batch, quad_y) in buf.text_batches.iter().zip([0.0, 100.0]) {
        let quad_group = buf
            .groups
            .iter()
            .enumerate()
            .find(|(_, group)| {
                group
                    .quads
                    .range()
                    .any(|qi| buf.quads[qi].rect.min.y == quad_y)
            })
            .map(|(i, _)| i as u32)
            .expect("panel quad group");
        assert_eq!(
            batch.last_group + 1,
            quad_group,
            "closed-batch text must paint in the group just before the overlapping \
             quad at y={quad_y}",
        );
    }
}

/// Solid, sharp, stroke-less, pixel-aligned quads carry `FillKind::FAST_BIT`; alignment is on the physical rect, and translucency does not disqualify.
#[test]
fn quad_fast_path_flag_cases() {
    use crate::primitives::packed::fill_axis::FillAxis;
    use crate::primitives::packed::fill_kind::FillKind;
    use crate::primitives::paint::brush::gradient::Spread;
    use crate::primitives::paint::lut_row::LutRow;

    let solid = |c: RgbaF32| BrushSource::Solid(c.into());
    let opaque = RgbaF32::srgb(0.5, 0.5, 0.5);

    let gradient = BrushSource::Gradient(ResolvedGradient {
        axis: FillAxis::ZERO,
        lut_row: LutRow::FALLBACK,
        kind: FillKind::linear(Spread::Pad),
    });
    let cases: &[(&str, Rect, Corners, Stroke, BrushSource, f32, bool)] = &[
        (
            "aligned sharp strokeless solid",
            Rect::new(10.0, 10.0, 20.0, 20.0),
            Corners::ZERO,
            Stroke::NONE,
            solid(opaque),
            1.0,
            true,
        ),
        (
            "translucent still qualifies",
            Rect::new(10.0, 10.0, 20.0, 20.0),
            Corners::ZERO,
            Stroke::NONE,
            solid(RgbaF32::srgba(0.5, 0.5, 0.5, 0.5)),
            1.0,
            true,
        ),
        (
            "fractional logical rect aligned at DPR 2",
            Rect::new(10.5, 10.5, 20.0, 20.0),
            Corners::ZERO,
            Stroke::NONE,
            solid(opaque),
            2.0,
            true,
        ),
        (
            "fractional rect disqualifies",
            Rect::new(10.25, 10.0, 20.0, 20.0),
            Corners::ZERO,
            Stroke::NONE,
            solid(opaque),
            1.0,
            false,
        ),
        (
            "fractional size disqualifies",
            Rect::new(10.0, 10.0, 20.5, 20.0),
            Corners::ZERO,
            Stroke::NONE,
            solid(opaque),
            1.0,
            false,
        ),
        (
            "corners disqualify",
            Rect::new(10.0, 10.0, 20.0, 20.0),
            Corners::all(4.0),
            Stroke::NONE,
            solid(opaque),
            1.0,
            false,
        ),
        (
            "stroke disqualifies",
            Rect::new(10.0, 10.0, 20.0, 20.0),
            Corners::ZERO,
            Stroke::new(RgbaF32::WHITE, 1.0),
            solid(opaque),
            1.0,
            false,
        ),
        (
            "gradient disqualifies",
            Rect::new(10.0, 10.0, 20.0, 20.0),
            Corners::ZERO,
            Stroke::NONE,
            gradient,
            1.0,
            false,
        ),
    ];

    for (name, r, corners, stroke, brush, dpr, expect_fast) in cases {
        let buf = run(
            |b, _arena| {
                QuadBuilder::new(*r)
                    .corners(*corners)
                    .brush(*brush)
                    .stroke(*stroke)
                    .draw(b);
            },
            &params_unsnapped(*dpr, UVec2::new(400, 400)),
        );
        assert_eq!(buf.quads.len(), 1, "{name}: quad emitted");
        let got = buf.quads[0].fill_kind;
        let plain = match brush {
            BrushSource::Solid(_) => FillKind::SOLID,
            BrushSource::Gradient(g) => g.kind,
        };
        let want = if *expect_fast {
            plain.with_fast()
        } else {
            plain
        };
        assert_eq!(got, want, "{name}: fill_kind");
    }
}

/// A labelled toolbar of eight non-overlapping icon+label buttons costs one icon batch and one text batch: a higher-kind draw closes the text batch only where it covers a run already in it.
#[test]
fn labelled_toolbar_costs_one_icon_batch_and_one_text_batch() {
    const BUTTONS: usize = 8;
    let out = run(
        |buf, _| {
            for i in 0..BUTTONS {
                let x = i as f32 * 100.0;
                icon(
                    buf,
                    Rect::new(x, 0.0, 16.0, 16.0),
                    IconRef::fixture(0, i as u16),
                );
                text(buf, Rect::new(x + 20.0, 0.0, 60.0, 16.0));
            }
        },
        &params(1.0, UVec2::new(1024, 64)),
    );

    assert_eq!(out.icons.len(), BUTTONS);
    assert_eq!(
        out.batches(PaintTier::Icon).len(),
        1,
        "every icon shares one atlas, so they are one draw however many buttons",
    );
    assert_eq!(
        out.text_batches.len(),
        1,
        "no icon covers a label, so the labels coalesce into one batch",
    );
    assert_eq!(out.groups.len(), 1, "disjoint draws need no group flush");
}

/// Control: an *image* between the labels coalesces identically; the saving belongs to the tier boundary.
#[test]
fn images_between_labels_coalesce_text_the_same_way() {
    const BUTTONS: usize = 8;
    let out = run(
        |buf, _| {
            for i in 0..BUTTONS {
                let x = i as f32 * 100.0;
                buf.draw_image(
                    ImageDraw {
                        payload: gpu_view_payload(Rect::new(x, 0.0, 16.0, 16.0), TextureId(1)),
                        view: None,
                    },
                    1.0,
                );
                text(buf, Rect::new(x + 20.0, 0.0, 60.0, 16.0));
            }
        },
        &params(1.0, UVec2::new(1024, 64)),
    );
    assert_eq!(out.text_batches.len(), 1);
}

/// Both halves of the ordering the coalescing rests on: an icon landing on the previous label splits the batch, a label landing on the icon flushes the group. Read twice; a composer that closed nothing fails the second arm.
#[test]
fn icon_over_prior_label_splits_batch_and_over_later_label_flushes_group() {
    #[derive(Debug)]
    struct Case {
        trailing_x: f32,
        groups: usize,
    }
    for case in [
        Case {
            trailing_x: 70.0,
            groups: 1,
        },
        Case {
            trailing_x: 50.0,
            groups: 2,
        },
    ] {
        let out = run(
            |buf, _| {
                text(buf, Rect::new(0.0, 0.0, 60.0, 16.0));
                icon(
                    buf,
                    Rect::new(40.0, 0.0, 16.0, 16.0),
                    IconRef::fixture(0, 0),
                );
                text(buf, Rect::new(case.trailing_x, 0.0, 60.0, 16.0));
            },
            &params(1.0, UVec2::new(1024, 64)),
        );
        assert_eq!(
            out.text_batches.len(),
            2,
            "the icon covers the first label in both arms: {case:?}",
        );
        assert_eq!(out.groups.len(), case.groups, "{case:?}");
    }
}

/// A batch surviving a higher-kind draw drains past the group flush: one batch, two groups, its `last_group` the second and the image's the first. Unobservable, since the two don't meet.
#[test]
fn text_batch_drains_past_a_non_overlapping_image() {
    let out = run(
        |buf, _| {
            text(buf, Rect::new(0.0, 0.0, 60.0, 16.0));
            buf.draw_image(
                ImageDraw {
                    payload: gpu_view_payload(Rect::new(100.0, 0.0, 16.0, 16.0), TextureId(1)),
                    view: None,
                },
                1.0,
            );
            // Over the image, so this run flushes the group.
            text(buf, Rect::new(110.0, 0.0, 60.0, 16.0));
        },
        &params(1.0, UVec2::new(1024, 64)),
    );
    assert_eq!(out.groups.len(), 2);
    assert_eq!(out.text_batches.len(), 1, "the image covers neither label");
    assert_eq!(out.text_batches[0].texts.len, 2);
    assert_eq!(out.text_batches[0].last_group, 1);
    assert_eq!(out.batches(PaintTier::Image).len(), 1);
    assert_eq!(
        out.batches(PaintTier::Image)[0].last_group,
        0,
        "the image drains a group before the batch that paints over it",
    );
}

/// A text batch's scissor never cuts the glyphs' extent. Pixel-snapped, a run at x 10 of width 100.4 spans 10..110.4; padded 0.25 % a side and covered: x 9..111, y 9..31.
///
/// Ink past the block (3, 2, 5, 4 px out) gives 7..115.4 by 8..34, padded 0.271 x 0.065 to 6.729..115.671 by 7.935..34.065, covered x 6..116, y 7..35. At scale 2: 216.8 x 52 from (14, 16), padded 0.542 x 0.13: x 13..232, y 15..69.
#[test]
fn a_text_scissor_covers_the_snapped_glyph_block() {
    let block = Rect::new(10.0, 10.0, 100.4, 20.0);
    let ink = Spacing::new(3.0, 2.0, 5.0, 4.0);
    let cases = [
        (
            1.0,
            UVec2::new(200, 100),
            Spacing::ZERO,
            10.0,
            URect::new(9, 9, 102, 22),
        ),
        (
            1.0,
            UVec2::new(200, 100),
            ink,
            10.0,
            URect::new(6, 7, 110, 28),
        ),
        (
            2.0,
            UVec2::new(400, 200),
            ink,
            20.0,
            URect::new(13, 15, 219, 54),
        ),
    ];
    for (scale, physical, ink, origin, scissor) in cases {
        let mut display = params(scale, physical);
        display.pixel_snap = true;
        let buf = run(|b, _| inked_text(b, block, ink), &display);
        assert_eq!(buf.text_batches.len(), 1);
        assert_eq!(buf.texts[0].origin, Vec2::splat(origin), "{scale} {ink:?}");
        assert_eq!(buf.text_batches[0].scissor, scissor, "{scale} {ink:?}");
    }
}
