//! Which draws share a group and a batch, and what forces a split.

use crate::common::span::Span;
use crate::internals::paint_capture::PaintCapture;
use crate::primitives::geometry::corners::Corners;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::geometry::spacing::Spacing;
use crate::primitives::geometry::translate_scale::TranslateScale;
use crate::primitives::geometry::urect::URect;
use crate::primitives::identity::icon_ref::IconRef;
use crate::primitives::identity::texture_id::TextureId;
use crate::primitives::paint::border::Border;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::shadow::Shadow;
use crate::renderer::frontend::composer::tests::quad_builder::QuadBuilder;
use crate::renderer::frontend::composer::tests::support::{
    TestPolyline, clip, clip_rounded, curve, draw, effect, icon, image, image_payload, inked_text,
    lanes, listed, mesh, params, params_unsnapped, rows_of, run, text, text_runs,
};
use crate::renderer::frontend::paint_sink::PaintSink;
use crate::renderer::frontend::payload::brush_source::BrushSource;
use crate::renderer::frontend::payload::draw_shadow_payload::DrawShadowPayload;
use crate::renderer::frontend::payload::draw_text_payload::DrawTextPayload;
use crate::renderer::frontend::payload::spin::Spin;
use crate::renderer::render_buffer::draw_run::Lane;
use crate::renderer::render_buffer::paint_tier::PaintTier;
use crate::renderer::render_buffer::text_draw_row::TextClip;
use crate::shape::geometry::box_geometry::PlacedBox;
use crate::shape::paint::lowered_shadow::LoweredShadow;
use crate::text::key::TextShapeKey;
use crate::text::shaped_ref::ShapedTextRef;
use glam::{UVec2, Vec2};
use std::f32::consts::{FRAC_PI_2, FRAC_PI_6};

/// Quads and text share a group until a quad lands on text before it, which lists after its
/// group's quads: that quad starts a new group, and it closes the open batch it overlaps, so later
/// text batches anew.
#[test]
fn a_quad_over_earlier_text_starts_a_group() {
    /// A label, the draws, the viewport's side, and the runs listed, in paint order.
    type Row<'a> = (&'a str, fn(&mut PaintCapture), u32, &'a [(Lane, Span)]);
    let rows: &[Row<'_>] = &[
        (
            "quads then text",
            |b| {
                draw(b, Rect::new(0.0, 0.0, 100.0, 100.0));
                draw(b, Rect::new(2.0, 2.0, 96.0, 96.0));
                text(b, Rect::new(10.0, 10.0, 80.0, 20.0));
            },
            200,
            &[(Lane::Quad, Span::new(0, 2)), (Lane::Text, Span::new(0, 1))],
        ),
        (
            "consecutive texts",
            |b| {
                draw(b, Rect::new(0.0, 0.0, 100.0, 100.0));
                text(b, Rect::new(10.0, 10.0, 80.0, 20.0));
                text(b, Rect::new(10.0, 35.0, 80.0, 20.0));
            },
            200,
            &[(Lane::Quad, Span::new(0, 1)), (Lane::Text, Span::new(0, 2))],
        ),
        (
            "five disjoint rows of a quad and its text",
            |b| {
                for i in 0..5 {
                    let y = (i as f32) * 40.0;
                    draw(b, Rect::new(0.0, y, 100.0, 28.0));
                    text(b, Rect::new(4.0, y + 4.0, 90.0, 20.0));
                }
            },
            400,
            &[(Lane::Quad, Span::new(0, 5)), (Lane::Text, Span::new(0, 5))],
        ),
        (
            "a quad over the text",
            |b| {
                draw(b, Rect::new(0.0, 0.0, 100.0, 100.0));
                text(b, Rect::new(10.0, 10.0, 80.0, 20.0));
                draw(b, Rect::new(20.0, 20.0, 60.0, 40.0));
            },
            200,
            &[
                (Lane::Quad, Span::new(0, 1)),
                (Lane::Text, Span::new(0, 1)),
                (Lane::Quad, Span::new(1, 1)),
            ],
        ),
        (
            "the next node's chrome over a label",
            |b| {
                draw(b, Rect::new(0.0, 0.0, 100.0, 28.0));
                text(b, Rect::new(4.0, 4.0, 90.0, 20.0));
                draw(b, Rect::new(40.0, 10.0, 100.0, 28.0));
                text(b, Rect::new(44.0, 14.0, 90.0, 20.0));
            },
            400,
            &[
                (Lane::Quad, Span::new(0, 1)),
                (Lane::Text, Span::new(0, 1)),
                (Lane::Quad, Span::new(1, 1)),
                (Lane::Text, Span::new(1, 1)),
            ],
        ),
        (
            "a redundant nested clip keeps the overlap state",
            |b| {
                clip(b, Rect::new(0.0, 0.0, 200.0, 200.0));
                draw(b, Rect::new(0.0, 0.0, 100.0, 28.0));
                text(b, Rect::new(4.0, 4.0, 90.0, 20.0));
                clip(b, Rect::new(0.0, 0.0, 200.0, 200.0));
                b.pop_clip();
                draw(b, Rect::new(40.0, 10.0, 100.0, 28.0));
                b.pop_clip();
            },
            400,
            &[
                (Lane::Quad, Span::new(0, 1)),
                (Lane::Text, Span::new(0, 1)),
                (Lane::Quad, Span::new(1, 1)),
            ],
        ),
    ];
    for &(label, build, side, want) in rows {
        let buf = run(|b, _| build(b), &params(1.0, UVec2::splat(side)));
        assert_eq!(listed(&buf), want, "{label}");
    }
}

/// A quad over text still in the open batch, whose last group is already closed, closes the batch
/// and nothing more: the text lists as it closes, after group 0's image and below the quads of
/// group 1. The flush to group 1 comes from the quad over the image.
#[test]
fn a_quad_over_text_that_already_rendered_does_not_flush() {
    let buf = run(
        |b, _arena| {
            text(b, Rect::new(0.0, 0.0, 50.0, 20.0));
            image(b, Rect::new(100.0, 0.0, 40.0, 40.0));
            draw(b, Rect::new(100.0, 0.0, 40.0, 40.0)); // over the image: flushes group 0
            draw(b, Rect::new(0.0, 0.0, 50.0, 20.0)); // over the text of group 0
        },
        &params(1.0, UVec2::new(200, 100)),
    );
    assert_eq!(
        listed(&buf),
        [
            (Lane::Tier(PaintTier::Image), Span::new(0, 1)),
            (Lane::Text, Span::new(0, 1)),
            (Lane::Quad, Span::new(0, 2)),
        ],
        "no flush for the second quad: both draw in group 1"
    );
}

/// A quad over text whose last group is already closed, and over an image of the open group,
/// closes the batch, which lists then, and flushes: the image lists at the end of group 1, so the
/// quad goes to group 2, above it.
#[test]
fn a_quad_over_closed_text_and_an_open_image_flushes() {
    let buf = run(
        |b, _arena| {
            text(b, Rect::new(0.0, 0.0, 50.0, 20.0));
            image(b, Rect::new(100.0, 0.0, 40.0, 40.0));
            draw(b, Rect::new(100.0, 0.0, 40.0, 40.0)); // over the image: flushes group 0
            image(b, Rect::new(150.0, 0.0, 40.0, 40.0)); // in group 1
            draw(b, Rect::new(0.0, 0.0, 200.0, 40.0)); // over the text and the second image
        },
        &params(1.0, UVec2::new(200, 100)),
    );
    assert_eq!(
        listed(&buf),
        [
            (Lane::Tier(PaintTier::Image), Span::new(0, 1)),
            (Lane::Text, Span::new(0, 1)),
            (Lane::Quad, Span::new(0, 1)),
            (Lane::Tier(PaintTier::Image), Span::new(1, 1)),
            (Lane::Quad, Span::new(1, 1)),
        ],
        "the last quad flushes group 1"
    );
}

/// A quad turned 30° about its center reaches the AA half width past each edge in its own frame:
/// (10, 10)..(30, 20) shades (8, 4)..(32, 26), as `QuadKey::shaded_rect` says. An image in row 25,
/// which only that fringe reaches, comes before it, so the quad flushes to paint above it.
#[test]
fn a_turned_quad_orders_against_its_fringe() {
    let buf = run(
        |b, _arena| {
            image(b, Rect::new(18.0, 25.0, 4.0, 1.0));
            QuadBuilder::new(Rect::new(10.0, 10.0, 20.0, 10.0))
                .turned(FRAC_PI_6)
                .draw(b);
        },
        &params(1.0, UVec2::new(100, 100)),
    );
    assert_eq!(
        buf.quads.instance()[0].header.shaded_rect(),
        Rect::new(8.0, 4.0, 24.0, 22.0)
    );
    assert_eq!(
        listed(&buf),
        [
            (Lane::Tier(PaintTier::Image), Span::new(0, 1)),
            (Lane::Quad, Span::new(0, 1)),
        ],
        "the quad flushes past the image"
    );
}

/// A quad 20 px square at x 10..30, turned a quarter about (50, 50), lands at x 40..60, which
/// the clip x 40..60 shows: the cull reads the pixels it reaches turned, (39.5, 9.5)..(60.5, 30.5)
/// grown by its fringe and clipped, so it is kept. Unturned, the same quad lies outside the clip
/// and is culled.
#[test]
fn a_turned_quad_is_culled_by_the_pixels_it_reaches() {
    let quad = QuadBuilder::new(Rect::new(10.0, 40.0, 20.0, 20.0));
    let quads = |quad: QuadBuilder| {
        run(
            |b, _arena| {
                clip(b, Rect::new(40.0, 0.0, 20.0, 100.0));
                quad.draw(b);
                b.pop_clip();
            },
            &params(1.0, UVec2::new(100, 100)),
        )
        .quads
        .len()
    };
    assert_eq!(
        quads(quad.turned_about(Vec2::new(50.0, 50.0), FRAC_PI_2)),
        1,
        "turned into the clip"
    );
    assert_eq!(quads(quad), 0, "unturned, outside it");
}

/// A quad orders by the whole pixels it shades, `QuadKey::shaded_rect`: one at x 10..20 shades
/// columns 10..20 and leaves an image at x 20..30 in its group; one at x 10..20.25 shades column
/// 20 too, so it closes the group to paint above the image recorded before it.
#[test]
fn a_quad_orders_by_the_pixels_it_shades() {
    let images = Lane::Tier(PaintTier::Image);
    for (width, lanes_listed) in [(10.0, [Lane::Quad, images]), (10.25, [images, Lane::Quad])] {
        let buf = run(
            |b, _arena| {
                image(b, Rect::new(20.0, 0.0, 10.0, 10.0));
                draw(b, Rect::new(10.0, 0.0, width, 10.0));
            },
            &params_unsnapped(1.0, UVec2::new(100, 100)),
        );
        assert_eq!(
            buf.quads.instance()[0].header.shaded_rect(),
            Rect::new(10.0, 0.0, if width == 10.0 { 10.0 } else { 11.0 }, 10.0)
        );
        assert_eq!(lanes(&buf), lanes_listed, "width {width}");
    }
}

#[test]
fn compose_shadow_outer_halo_after_text_splits_group() {
    let sigma = 4.0;
    // The composer grows the source by 4σ = 16, to x = 34: past the text's 39.
    let source = Rect::new(50.0, 50.0, 50.0, 50.0);
    let buf = run(
        |b, _arena| {
            text(b, Rect::new(39.0, 60.0, 2.0, 10.0));
            b.draw(
                DrawShadowPayload::new(
                    PlacedBox {
                        rect: source,
                        corners: Corners::ZERO,
                    },
                    LoweredShadow::lower(Shadow::drop(RgbaF32::BLACK, Vec2::ZERO, sigma)),
                ),
                1.0,
            );
        },
        &params(1.0, UVec2::new(200, 200)),
    );

    assert_eq!(
        listed(&buf),
        [(Lane::Text, Span::new(0, 1)), (Lane::Quad, Span::new(0, 1))],
        "outer halo overlap must split"
    );
}

/// Rows in separate clips, which cut none of them, draw as one quad run and one text batch.
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
    assert_eq!(
        listed(&buf),
        [(Lane::Quad, Span::new(0, 2)), (Lane::Text, Span::new(0, 2))],
        "clips that cut nothing cost no run, and the rows share one text batch",
    );
}

/// A clipped text run clips its own glyphs to its bounds, so it joins its wider neighbor's batch,
/// whose scissor is the union of both runs' bounds; its row keeps its clipped bounds,
/// (40, 40, 20 × 20). A turned run cannot clip its glyphs on the CPU, so a turned clipped one
/// still takes a batch of its own, its scissor its clipped bounds.
#[test]
fn a_clipped_text_run_batches_unless_it_turns() {
    let turned = |spin: Option<Spin>| {
        run(
            |b, _arena| {
                text(b, Rect::new(10.0, 10.0, 200.0, 20.0));
                clip(b, Rect::new(40.0, 40.0, 20.0, 20.0));
                b.draw(
                    DrawTextPayload {
                        rect: Rect::new(40.0, 40.0, 100.0, 20.0),
                        ink: Spacing::ZERO,
                        color: RgbaF32::WHITE.to_f16(),
                        text: ShapedTextRef {
                            key: TextShapeKey::fixture(),
                            span: Span::default(),
                        },
                        spin,
                    },
                    1.0,
                );
                b.pop_clip();
            },
            &params(1.0, UVec2::new(300, 300)),
        )
    };
    let still = turned(None);
    assert_eq!(still.texts[1].bounds, URect::new(40, 40, 20, 20));
    assert_eq!(
        still.texts[0].reach,
        TextClip::Whole,
        "off the viewport's edge, which its padded reach would cross at 0"
    );
    assert_eq!(
        still.texts[1].reach,
        TextClip::Cut,
        "the clip cuts its glyphs"
    );
    let union = still.texts[0].bounds.union(still.texts[1].bounds);
    assert_eq!(text_runs(&still), [(Span::new(0, 2), union)]);
    assert_eq!(
        still.runs[0].scissor, None,
        "unturned glyphs clip themselves"
    );

    let spun = turned(Some(Spin {
        pivot: Vec2::new(50.0, 50.0),
        angle: 0.5,
    }));
    let scissors: Vec<Option<URect>> = spun.runs.iter().map(|run| run.scissor).collect();
    assert_eq!(
        scissors,
        [None, Some(URect::new(40, 40, 20, 20))],
        "the turned run draws under its bounds"
    );
    let spun = text_runs(&spun);
    assert_eq!(spun.len(), 2);
    assert_eq!(spun[1], (Span::new(1, 1), URect::new(40, 40, 20, 20)));
}

/// The viewport's edge cuts a run's glyphs as a clip does, but needs no scissor of the batch: two
/// turned runs, one inside the 300 px viewport and one past its right edge, both reach `Cut` only
/// for the second, and share one batch, as identical bounds bind only a run a clip cuts.
#[test]
fn the_viewport_edge_cuts_a_run_but_not_its_batch() {
    let turned_text = |b: &mut PaintCapture, rect: Rect| {
        b.draw(
            DrawTextPayload {
                rect,
                ink: Spacing::ZERO,
                color: RgbaF32::WHITE.to_f16(),
                text: ShapedTextRef {
                    key: TextShapeKey::fixture(),
                    span: Span::default(),
                },
                spin: Some(Spin {
                    pivot: rect.center(),
                    angle: 0.5,
                }),
            },
            1.0,
        );
    };
    let buf = run(
        |b, _arena| {
            turned_text(b, Rect::new(40.0, 40.0, 100.0, 20.0));
            turned_text(b, Rect::new(240.0, 140.0, 100.0, 20.0));
        },
        &params(1.0, UVec2::new(300, 300)),
    );
    assert_eq!(buf.texts[0].reach, TextClip::Whole);
    assert_eq!(buf.texts[1].reach, TextClip::Cut);
    assert_eq!(buf.texts[1].bounds.max().x, 300, "the viewport clamps it");
    let union = buf.texts[0].bounds.union(buf.texts[1].bounds);
    assert_eq!(text_runs(&buf), [(Span::new(0, 2), union)]);
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
        text_runs(&buf).len(),
        1,
        "two strict runs with identical clip bounds should share a batch",
    );
}

/// A rounded-clip change keeps the text batch: each run carries its own chain, which each of its
/// glyphs takes, so runs under different rounded clips draw in one batch.
#[test]
fn compose_rounded_clip_change_keeps_the_text_batch() {
    let buf = run(
        |b, _arena| {
            // Each run crosses its clip's right edge, so it takes the clip's chain.
            clip_rounded(b, Rect::new(0.0, 0.0, 100.0, 30.0), Corners::all(4.0));
            text(b, Rect::new(14.0, 4.0, 90.0, 20.0));
            b.pop_clip();
            clip_rounded(b, Rect::new(0.0, 40.0, 100.0, 30.0), Corners::all(8.0));
            text(b, Rect::new(14.0, 44.0, 90.0, 20.0));
            b.pop_clip();
        },
        &params(1.0, UVec2::new(200, 200)),
    );
    assert_eq!(
        rows_of(&buf, Lane::Text),
        [Span::new(0, 2)],
        "a rounded change keeps the batch"
    );
    let chain = |t: usize| &buf.rounded_clips[buf.texts[t].clip.span().range()];
    assert_eq!(chain(0).len(), 1);
    assert_eq!(chain(1).len(), 1);
    assert_eq!(chain(0)[0].corners.top_left(), 4.0);
    assert_eq!(chain(1)[0].corners.top_left(), 8.0);
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
        text_runs(&buf).len(),
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
        text_runs(&buf).len(),
        2,
        "quad overlapping prior batch text must split the batch",
    );
}

#[test]
fn tight_curve_bound_avoids_false_group_split() {
    #[derive(Debug)]
    struct Case {
        image_x: f32,
        lanes: [Lane; 2],
    }

    // The curve ends at x=20; width 2 + 0.5 AA bounds it at ceil(21.5)=22. x=22 is disjoint; one pixel left overlaps.
    let cases = [
        Case {
            image_x: 22.0,
            lanes: [Lane::Tier(PaintTier::Image), Lane::Tier(PaintTier::Curve)],
        },
        Case {
            image_x: 21.0,
            lanes: [Lane::Tier(PaintTier::Curve), Lane::Tier(PaintTier::Image)],
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
        assert_eq!(lanes(&buf), case.lanes, "{case:?}");
    }
}

/// Every ordered pair of tiers, over and apart: a group lists its tiers in `PaintTier::ALL` order,
/// so a draw that overlaps one of a later tier recorded before it starts a new group, listed
/// after the first draw's, and any other pair shares one, listed in tier order. The first draw covers (10, 10)..(40, 40), the
/// overlapping second (20, 20)..(50, 50) and the apart one (120, 120)..(150, 150); a mesh's bounds
/// grow 1 px and a curve's 2 px, which keep each pair as it is. Two draws of one tier take one
/// run.
#[test]
fn a_draw_under_an_earlier_tier_starts_a_group() {
    let first_rect = Rect::new(10.0, 10.0, 30.0, 30.0);
    let draw_tier = |b: &mut PaintCapture, tier: PaintTier, r: Rect| match tier {
        PaintTier::Mesh => mesh(b, r),
        PaintTier::Image => image(b, r),
        PaintTier::Icon => icon(b, r, IconRef::fixture(0, 0)),
        PaintTier::Curve => curve(b, r),
        PaintTier::Effect => effect(b, r),
    };
    for first in PaintTier::ALL {
        for second in PaintTier::ALL {
            for (apart, second_rect) in [
                (false, Rect::new(20.0, 20.0, 30.0, 30.0)),
                (true, Rect::new(120.0, 120.0, 30.0, 30.0)),
            ] {
                let buf = run(
                    |b, _| {
                        draw_tier(b, first, first_rect);
                        draw_tier(b, second, second_rect);
                    },
                    &params(1.0, UVec2::new(200, 200)),
                );
                let split = !apart && second < first;
                let label = format!("{first:?} then {second:?}, apart: {apart}");
                let want = if split {
                    vec![first, second]
                } else if first == second {
                    vec![first]
                } else {
                    vec![first.min(second), first.max(second)]
                };
                let want: Vec<Lane> = want.into_iter().map(Lane::Tier).collect();
                assert_eq!(lanes(&buf), want, "{label}");
            }
        }
    }
}

/// A quad overlapping text in an already-closed batch of the same group must still flush: each
/// batch, closed by the polyline over it, waits on its group and lists after its quads, before
/// the quad over it.
#[test]
fn quad_flushes_text_in_already_closed_batch_same_group() {
    let buf = run(
        |b, store| {
            text(b, Rect::new(0.0, 0.0, 300.0, 20.0));
            TestPolyline::new(&[Vec2::new(200.0, 10.0), Vec2::new(300.0, 10.0)], 1.0)
                .record(b, store);
            draw(b, Rect::new(0.0, 0.0, 100.0, 60.0));
            text(b, Rect::new(0.0, 100.0, 300.0, 20.0));
            TestPolyline::new(&[Vec2::new(200.0, 110.0), Vec2::new(300.0, 110.0)], 1.0)
                .record(b, store);
            draw(b, Rect::new(0.0, 100.0, 100.0, 60.0));
        },
        &params(1.0, UVec2::new(600, 600)),
    );
    let curve = Lane::Tier(PaintTier::Curve);
    assert_eq!(
        listed(&buf),
        [
            (Lane::Text, Span::new(0, 1)),
            (curve, Span::new(0, 1)),
            (Lane::Quad, Span::new(0, 1)),
            (Lane::Text, Span::new(1, 1)),
            (curve, Span::new(1, 1)),
            (Lane::Quad, Span::new(1, 1)),
        ],
        "each panel quad paints over the text before it"
    );
    // Each box is padded 0.25 % a side (0.75 px across, 0.05 down) and covered; the first clamps to (0, 0)..(301, 21), the second spans y 99.95..120.05, so 99..121.
    let scissors: Vec<URect> = text_runs(&buf).iter().map(|run| run.1).collect();
    assert_eq!(
        scissors,
        [URect::new(0, 0, 301, 21), URect::new(0, 99, 301, 22)]
    );
}

/// An opaque solid box whose physical edges lie on whole pixels, sharp, covers its own rect whole,
/// so it drops an under quad of the same rect, whatever its border, as its fill reaches under it;
/// any other keeps a half-pixel ramp inside its edge, which the under quad's pixels reach past.
/// Alignment is on the physical rect.
#[test]
fn a_pixel_aligned_sharp_box_covers_its_own_rect() {
    let opaque = BrushSource::Solid(RgbaF32::srgb(0.5, 0.5, 0.5).to_f16());
    let cases: &[(&str, Rect, Corners, Border, f32, bool)] = &[
        (
            "aligned sharp borderless",
            Rect::new(10.0, 10.0, 20.0, 20.0),
            Corners::ZERO,
            Border::NONE,
            1.0,
            true,
        ),
        (
            "fractional logical rect aligned at DPR 2",
            Rect::new(10.5, 10.5, 20.0, 20.0),
            Corners::ZERO,
            Border::NONE,
            2.0,
            true,
        ),
        (
            "fractional rect",
            Rect::new(10.25, 10.0, 20.0, 20.0),
            Corners::ZERO,
            Border::NONE,
            1.0,
            false,
        ),
        (
            "fractional size",
            Rect::new(10.0, 10.0, 20.5, 20.0),
            Corners::ZERO,
            Border::NONE,
            1.0,
            false,
        ),
        (
            "corners",
            Rect::new(10.0, 10.0, 20.0, 20.0),
            Corners::all(4.0),
            Border::NONE,
            1.0,
            false,
        ),
        (
            "an opaque border",
            Rect::new(10.0, 10.0, 20.0, 20.0),
            Corners::ZERO,
            Border::new(RgbaF32::WHITE, 1.0),
            1.0,
            true,
        ),
        (
            "a translucent border",
            Rect::new(10.0, 10.0, 20.0, 20.0),
            Corners::ZERO,
            Border::new(RgbaF32::new(1.0, 1.0, 1.0, 0.5), 1.0),
            1.0,
            true,
        ),
        (
            "a translucent border on a fractional rect",
            Rect::new(10.25, 10.0, 20.0, 20.0),
            Corners::ZERO,
            Border::new(RgbaF32::new(1.0, 1.0, 1.0, 0.5), 1.0),
            1.0,
            false,
        ),
    ];

    for &(name, r, corners, border, dpr, covers) in cases {
        let buf = run(
            |b, _arena| {
                QuadBuilder::new(r).brush(opaque).draw(b);
                QuadBuilder::new(r)
                    .corners(corners)
                    .brush(opaque)
                    .border(border)
                    .draw(b);
            },
            &params_unsnapped(dpr, UVec2::new(400, 400)),
        );
        let quads = if covers { 1 } else { 2 };
        assert_eq!(buf.quads.len(), quads, "{name}");
    }
}

/// A labeled toolbar of eight non-overlapping icon+label buttons costs one icon batch and one text batch: a higher-kind draw closes the text batch only where it covers a run already in it.
#[test]
fn labeled_toolbar_costs_one_icon_batch_and_one_text_batch() {
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
        listed(&out),
        [
            (Lane::Text, Span::new(0, 8)),
            (Lane::Tier(PaintTier::Icon), Span::new(0, 8)),
        ],
        "every icon shares one atlas and no icon covers a label, so one group draws one run of \
         each, however many buttons",
    );
}

/// Control: an *image* between the labels coalesces identically; the saving belongs to the tier boundary.
#[test]
fn images_between_labels_coalesce_text_the_same_way() {
    const BUTTONS: usize = 8;
    let out = run(
        |buf, _| {
            for i in 0..BUTTONS {
                let x = i as f32 * 100.0;
                buf.draw(
                    image_payload(Rect::new(x, 0.0, 16.0, 16.0), TextureId(1)),
                    1.0,
                );
                text(buf, Rect::new(x + 20.0, 0.0, 60.0, 16.0));
            }
        },
        &params(1.0, UVec2::new(1024, 64)),
    );
    assert_eq!(text_runs(&out).len(), 1);
}

/// Both halves of the ordering the coalescing rests on: an icon landing on the previous label splits the batch, a label landing on the icon flushes the group. Read twice; a composer that closed nothing fails the second arm.
#[test]
fn icon_over_prior_label_splits_batch_and_over_later_label_flushes_group() {
    #[derive(Debug)]
    struct Case {
        trailing_x: f32,
        lanes: [Lane; 3],
    }
    for case in [
        Case {
            trailing_x: 70.0,
            lanes: [Lane::Text, Lane::Text, Lane::Tier(PaintTier::Icon)],
        },
        Case {
            trailing_x: 50.0,
            lanes: [Lane::Text, Lane::Tier(PaintTier::Icon), Lane::Text],
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
            lanes(&out),
            case.lanes,
            "the icon covers the first label in both arms, and the second flushes past it in \
             one: {case:?}",
        );
    }
}

/// A batch surviving a higher-kind draw lists past the group flush: one batch, two groups, the image listed in the first and the batch at the end of the second. Unobservable, since the two don't meet.
#[test]
fn text_batch_drains_past_a_non_overlapping_image() {
    let out = run(
        |buf, _| {
            text(buf, Rect::new(0.0, 0.0, 60.0, 16.0));
            buf.draw(
                image_payload(Rect::new(100.0, 0.0, 16.0, 16.0), TextureId(1)),
                1.0,
            );
            // Over the image, so this run flushes the group.
            text(buf, Rect::new(110.0, 0.0, 60.0, 16.0));
        },
        &params(1.0, UVec2::new(1024, 64)),
    );
    assert_eq!(
        listed(&out),
        [
            (Lane::Tier(PaintTier::Image), Span::new(0, 1)),
            (Lane::Text, Span::new(0, 2)),
        ],
        "the image covers neither label, and lists a group before the batch that paints over it",
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
        assert_eq!(buf.texts[0].origin, Vec2::splat(origin), "{scale} {ink:?}");
        assert_eq!(
            text_runs(&buf),
            [(Span::new(0, 1), scissor)],
            "{scale} {ink:?}"
        );
    }

    // Zoomed out to 0.5 the slack is still a share of the local size: a block 101.6 x 20 spans
    // 5..55.8 by 5..15, padded `101.6 · 0.0025 = 0.254` x `0.05` to 4.746..56.054 by 4.95..15.05,
    // covered x 4..57, y 4..16. A share of the 50.8 on screen, 0.127, would end at 55.927 and cut
    // the last column at 56.
    let mut display = params(1.0, UVec2::new(200, 100));
    display.pixel_snap = true;
    let buf = run(
        |b, _| {
            b.push_transform(TranslateScale::from_scale(0.5));
            inked_text(b, Rect::new(10.0, 10.0, 101.6, 20.0), Spacing::ZERO);
            b.pop_transform();
        },
        &display,
    );
    assert_eq!(
        text_runs(&buf),
        [(Span::new(0, 1), URect::new(4, 4, 53, 12))]
    );
}
