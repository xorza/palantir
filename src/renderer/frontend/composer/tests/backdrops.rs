//! Where a backdrop pane cuts the frame, and what its row records.

use crate::internals::paint_capture::PaintCapture;
use crate::primitives::geometry::corners::Corners;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::geometry::urect::URect;
use crate::primitives::identity::pane_key::PaneKey;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::paint::backdrop_filter::{BackdropFilter, Lens};
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::lut_row::LutRow;
use crate::renderer::frontend::composer::tests::support::{
    draw, draw_marked, params, run, survivor_calls, text,
};
use crate::renderer::frontend::paint_sink::PaintSink;
use crate::renderer::frontend::payload::draw_pane_payload::DrawPanePayload;
use crate::renderer::frontend::payload::draw_quad_payload::{BackdropQuad, DrawQuadPayload};
use crate::renderer::render_buffer::backdrop_row::PaneSource;
use crate::shape::paint::blur_plan::BlurPlan;
use crate::shape::paint::lowered_backdrop::LoweredBackdrop;
use crate::shape::paint::shape_stroke::ShapeStroke;
use crate::shape::rect::RectKind;
use glam::UVec2;

fn pane(buf: &mut PaintCapture, rect: Rect, sigma: f32, alpha: f32) {
    let backdrop = LoweredBackdrop::from(BackdropFilter::blur(sigma));
    let quad = DrawQuadPayload::backdrop(BackdropQuad {
        kind: RectKind::Rounded,
        rect,
        corners: Corners::ZERO,
        tint: RgbaF32::new(1.0, 1.0, 1.0, 0.25).to_f16(),
        stroke: ShapeStroke::NONE,
        backdrop,
    });
    let key = PaneKey {
        owner: WidgetId::from_hash("pane"),
        shape: buf.calls.len() as u32,
    };
    buf.draw_pane(
        DrawPanePayload {
            quad,
            key,
            backdrop,
        },
        alpha,
    );
}

/// A pane closes the group and the text batch before it, so its barrier is the first group after
/// them and its quad opens that group; its row records the plan of its physical σ and its rect
/// grown by the plan's reach, inside the surface. At scale 2, σ 5 is 10 physical px: two levels,
/// and a pane at (20, 20)..(60, 40) logical reads (40, 40)..(120, 80) physical grown by the reach.
#[test]
fn a_pane_cuts_the_frame_at_its_barrier() {
    let buf = run(
        |b, _| {
            draw(b, Rect::new(0.0, 0.0, 90.0, 90.0));
            text(b, Rect::new(10.0, 10.0, 40.0, 10.0));
            pane(b, Rect::new(20.0, 20.0, 40.0, 20.0), 5.0, 1.0);
            draw(b, Rect::new(30.0, 30.0, 5.0, 5.0));
        },
        &params(2.0, UVec2::new(200, 200)),
    );
    assert_eq!(buf.groups.len(), 2);
    assert_eq!(buf.groups[0].quads.range(), 0..1);
    assert_eq!(buf.groups[1].quads.range(), 1..3);
    assert_eq!(buf.text_batches.len(), 1);
    assert_eq!(buf.text_batches[0].last_group, 0);
    let plan = BlurPlan::new(10.0);
    assert_eq!(plan.levels(), 2);
    let reach = plan.reach();
    assert_eq!(buf.backdrops.len(), 1);
    let row = buf.backdrops[0];
    assert_eq!(row.barrier, 1);
    assert_eq!(row.plan, plan);
    assert_eq!(
        row.footprint,
        URect::from_min_max(
            UVec2::splat(40 - reach.min(40)),
            UVec2::new(120 + reach, 80 + reach).min(UVec2::splat(200)),
        )
    );
    let quad = buf.quads[1];
    assert!(quad.fill_kind.is_backdrop());
    assert_eq!(quad.fill_lut_row, LutRow(0));
    assert_eq!(quad.fill_axis.lanes(), [0.0, 1.0, 0.0, 1.0]);
}

/// A pane past every plan and every lens coerces instead of overflowing: σ 10⁶ logical saturates
/// at 65504 where it lowers, and at scale 2 plans as `BlurPlan::MAX_SIGMA`; a lens of thickness and index 65504 reaches `u32::MAX`, so the footprint
/// is the whole surface.
#[test]
fn a_pane_past_every_plan_coerces_to_the_widest() {
    let backdrop = LoweredBackdrop::from(BackdropFilter::blur(1.0e6).with_lens(Lens {
        thickness: 65504.0,
        refractive_index: 65504.0,
        ..Lens::default()
    }));
    let buf = run(
        |b, _| {
            let quad = DrawQuadPayload::backdrop(BackdropQuad {
                kind: RectKind::Rounded,
                rect: Rect::new(20.0, 20.0, 40.0, 20.0),
                corners: Corners::ZERO,
                tint: RgbaF32::TRANSPARENT.to_f16(),
                stroke: ShapeStroke::NONE,
                backdrop,
            });
            let key = PaneKey {
                owner: WidgetId::from_hash("pane"),
                shape: 0,
            };
            b.draw_pane(
                DrawPanePayload {
                    quad,
                    key,
                    backdrop,
                },
                1.0,
            );
        },
        &params(2.0, UVec2::new(200, 200)),
    );
    assert_eq!(backdrop.sigma(), 65504.0);
    let row = buf.backdrops[0];
    assert_eq!(row.plan, BlurPlan::new(BlurPlan::MAX_SIGMA));
    assert_eq!(
        row.footprint,
        URect::from_min_max(UVec2::ZERO, UVec2::splat(200))
    );
}

/// A pane's plan takes its σ in f32, as the cascade's footprint does. σ 1639/256 at scale 1.25 is
/// 8.0029 physical px, past the 8 px edge of the second level: two levels. Rounded to f16 again
/// it would be 8.0 and one level, whose reach differs.
#[test]
fn a_panes_plan_takes_its_sigma_unrounded() {
    let sigma = 1639.0 / 256.0;
    let physical = sigma * 1.25;
    assert!(physical > 8.0);
    assert_eq!(BlurPlan::new(physical).levels(), 2);
    assert_eq!(BlurPlan::new(8.0).levels(), 1);
    let buf = run(
        |b, _| pane(b, Rect::new(20.0, 20.0, 20.0, 20.0), sigma, 1.0),
        &params(1.25, UVec2::new(200, 200)),
    );
    assert_eq!(buf.backdrops[0].plan, BlurPlan::new(physical));
}

/// A quad after a pane covering one before it leaves it alone: the pane's capture copies what it
/// painted. σ 4 at scale 1 reaches 16 px, so the pane's footprint takes in the first quad.
#[test]
fn no_prune_crosses_a_barrier() {
    let buf = run(
        |b, _| {
            draw_marked(b, Rect::new(0.0, 0.0, 100.0, 100.0));
            pane(b, Rect::new(105.0, 105.0, 10.0, 10.0), 4.0, 1.0);
            draw_marked(b, Rect::new(0.0, 0.0, 100.0, 100.0));
        },
        &params(1.0, UVec2::new(200, 200)),
    );
    assert_eq!(
        survivor_calls(&buf),
        [0, 255, 2],
        "the first quad, the white pane, the last"
    );
}

/// A paint animation's fade scales the pane's opacity lane, which fades the backdrop and the tint
/// together; a pane faded to nothing draws nothing and cuts nothing.
#[test]
fn a_fade_scales_the_panes_opacity() {
    let buf = run(
        |b, _| {
            pane(b, Rect::new(0.0, 0.0, 10.0, 10.0), 4.0, 0.5);
            pane(b, Rect::new(0.0, 0.0, 10.0, 10.0), 4.0, 0.0);
        },
        &params(1.0, UVec2::new(100, 100)),
    );
    assert_eq!(buf.backdrops.len(), 1);
    assert_eq!(buf.quads.len(), 1);
    assert_eq!(buf.quads[0].fill_axis.lanes()[3], 0.5);
    assert_eq!(
        RgbaF32::from(buf.quads[0].fill).a,
        0.125,
        "the tint's quarter alpha, halved"
    );
}

/// An opaque quad over the whole surface after a pane hides the pane: the clear fold drops its
/// quad and keeps its row, hidden, so it neither captures nor keeps a result that goes stale under
/// the cover.
#[test]
fn the_clear_fold_hides_the_panes_before_it() {
    let buf = run(
        |b, _| {
            pane(b, Rect::new(0.0, 0.0, 10.0, 10.0), 4.0, 1.0);
            draw(b, Rect::new(0.0, 0.0, 100.0, 100.0));
        },
        &params(1.0, UVec2::new(100, 100)),
    );
    let sources: Vec<PaneSource> = buf.backdrops.iter().map(|row| row.source).collect();
    assert_eq!(sources, [PaneSource::Hidden]);
    assert!(buf.quads.is_empty());
}

/// An opaque quad of a pane's group that covers the pane prunes its quad and hides its row; a
/// pane beside it keeps both, and its quad still reads its own row.
#[test]
fn a_pane_under_an_opaque_quad_is_hidden() {
    let buf = run(
        |b, _| {
            draw(b, Rect::new(0.0, 0.0, 10.0, 10.0));
            pane(b, Rect::new(20.0, 20.0, 20.0, 20.0), 1.0, 1.0);
            pane(b, Rect::new(60.0, 20.0, 20.0, 20.0), 1.0, 1.0);
            draw(b, Rect::new(10.0, 10.0, 40.0, 40.0));
        },
        &params(1.0, UVec2::new(100, 100)),
    );
    let sources: Vec<PaneSource> = buf.backdrops.iter().map(|row| row.source).collect();
    assert_eq!(sources, [PaneSource::Hidden, PaneSource::Held]);
    let panes: Vec<LutRow> = buf
        .quads
        .iter()
        .filter(|quad| quad.fill_kind.is_backdrop())
        .map(|quad| quad.fill_lut_row)
        .collect();
    assert_eq!(panes, [LutRow(1)]);
}

/// A pane with nothing composed before it stands at barrier 0, which the backend reads as a
/// backdrop of the clear: also after a quad over the whole surface folds into the clear. A pane
/// right after it has the first pane's group before it, so it captures what the first painted.
#[test]
fn a_pane_first_in_the_frame_has_nothing_before_it() {
    let buf = run(
        |b, _| {
            draw(b, Rect::new(0.0, 0.0, 100.0, 100.0));
            pane(b, Rect::new(10.0, 10.0, 20.0, 20.0), 4.0, 1.0);
            pane(b, Rect::new(32.0, 10.0, 20.0, 20.0), 4.0, 1.0);
        },
        &params(1.0, UVec2::new(100, 100)),
    );
    assert!(
        buf.clear_override.is_some(),
        "the cover folds into the clear"
    );
    let barriers: Vec<u32> = buf.backdrops.iter().map(|row| row.barrier).collect();
    assert_eq!(barriers, [0, 1]);
}

/// A pane whose footprint misses everything painted since the last break captures there: at 0,
/// the clear, when it misses everything; at the last pane's barrier when it misses that pane and
/// what followed. One that takes in the last pane breaks anew. σ 4 at scale 1 reaches 16 px.
#[test]
fn a_pane_shares_the_last_break_when_nothing_since_touches_it() {
    let buf = run(
        |b, _| {
            draw(b, Rect::new(0.0, 0.0, 40.0, 40.0));
            pane(b, Rect::new(100.0, 100.0, 20.0, 20.0), 4.0, 1.0);
            pane(b, Rect::new(10.0, 10.0, 20.0, 20.0), 4.0, 1.0);
            pane(b, Rect::new(10.0, 150.0, 20.0, 20.0), 4.0, 1.0);
            pane(b, Rect::new(40.0, 150.0, 20.0, 20.0), 4.0, 1.0);
        },
        &params(1.0, UVec2::new(200, 200)),
    );
    let barriers: Vec<u32> = buf.backdrops.iter().map(|row| row.barrier).collect();
    assert_eq!(
        barriers,
        [0, 1, 1, 2],
        "apart from the quad, on it, apart, beside the last"
    );
}
