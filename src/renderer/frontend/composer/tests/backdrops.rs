//! Where a backdrop cuts the frame, and what its row records.

use crate::cascade::plane_backdrop::PlaneBackdrop;
use crate::internals::paint_capture::PaintCapture;
use crate::primitives::geometry::corners::Corners;
use crate::primitives::geometry::coverage::Coverage;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::geometry::urect::URect;
use crate::primitives::identity::backdrop_key::BackdropKey;
use crate::primitives::identity::effect_id::EffectId;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::packed::fill_axis::FillAxis;
use crate::primitives::paint::color::RgbaF32;
use crate::renderer::backdrop_captures::BackdropTicket;
use crate::renderer::frontend::composer::tests::support::{
    draw, draw_marked, params, run, survivor_calls, text,
};
use crate::renderer::frontend::paint_sink::PaintSink;
use crate::renderer::frontend::payload::brush_source::BrushSource;
use crate::renderer::frontend::payload::draw_backdrop_payload::{
    BackdropDraw, DrawBackdropPayload,
};
use crate::renderer::render_buffer::RenderBuffer;
use crate::renderer::render_buffer::backdrop_row::{
    BackdropCapture, BackdropRead, BackdropRow, BackdropSource,
};
use crate::renderer::render_buffer::effect_draw_row::EffectInstance;
use crate::renderer::render_buffer::paint_tier::PaintTier;
use crate::shape::paint::blur_plan::BlurPlan;
use crate::shape::paint::lowered_border::LoweredBorder;
use crate::shape::paint::lowered_material::LoweredMaterial;
use glam::UVec2;

/// The ticket of backdrop `i` at `rect` and σ `sigma`, at scale 1, as the ledger writes it: its
/// footprint the rect grown by the plan's reach and a pixel.
fn ticket(i: u32, rect: Rect, sigma: f32, captures: bool) -> BackdropTicket {
    let plan = BlurPlan::new(sigma);
    let footprint = URect::covering(rect.inflated(PlaneBackdrop::reach(plan, 0)));
    BackdropTicket {
        capture: BackdropCapture {
            key: BackdropKey {
                owner: WidgetId::from_hash("backdrop"),
                shape: i,
            },
            footprint,
            plan,
            region: plan
                .region(footprint, footprint.max())
                .expect("a test backdrop paints"),
        },
        captures,
    }
}

/// A backdrop at `rect` under `ticket`, faded by `alpha`; the ticket holds its plan, so its own σ
/// is any.
fn ticketed(buf: &mut PaintCapture, rect: Rect, ticket: BackdropTicket, alpha: f32) {
    buf.draw(
        DrawBackdropPayload {
            coverage: Coverage::Inside,
            rect,
            corners: Corners::ZERO,
            spin: None,
            fill: BrushSource::Solid(RgbaF32::new(1.0, 1.0, 1.0, 0.25).to_f16()).gpu_fill(),
            fill_axis: FillAxis::ZERO,
            stroke: LoweredBorder::NONE,
            material: LoweredMaterial::fixture(4.0),
            opacity: 1.0,
            read: BackdropDraw::Capture(ticket),
        },
        alpha,
    );
}

/// The source of `row`, a backdrop's.
fn source(row: &BackdropRow) -> BackdropSource {
    match row.read {
        BackdropRead::Capture { source, .. } => source,
        BackdropRead::Ground => panic!("a backdrop, not the ground"),
    }
}

/// A capturing backdrop at `rect` and σ `sigma`, faded by `alpha`.
fn backdrop(buf: &mut PaintCapture, rect: Rect, sigma: f32, alpha: f32) {
    let i = buf.calls.len() as u32;
    ticketed(buf, rect, ticket(i, rect, sigma, true), alpha);
}

/// Each capturing row's barrier, `0` for one of the clear.
fn barriers(buf: &RenderBuffer) -> Vec<u32> {
    buf.backdrops
        .iter()
        .map(|row| match source(row) {
            BackdropSource::Capture { barrier } => barrier,
            BackdropSource::Clear => 0,
            source => panic!("a capturing backdrop, not {source:?}"),
        })
        .collect()
}

/// A capturing backdrop closes the group and the text batch before it, so its barrier is the first
/// group after them and its effect row opens that group, and the quad over it after it closes that
/// group in turn, as it paints over a draw that replays after quads; its row takes its ticket's
/// footprint, plan and region, and its effect row reads that row through its material's effect,
/// at its whole opacity, with no luminance bound.
#[test]
fn a_backdrop_cuts_the_frame_at_its_barrier() {
    let rect = Rect::new(20.0, 20.0, 40.0, 20.0);
    let ticket = ticket(0, rect, 5.0, true);
    let buf = run(
        |b, _| {
            draw(b, Rect::new(0.0, 0.0, 90.0, 90.0));
            text(b, Rect::new(10.0, 10.0, 40.0, 10.0));
            ticketed(b, rect, ticket, 1.0);
            draw(b, Rect::new(30.0, 30.0, 5.0, 5.0));
        },
        &params(1.0, UVec2::new(200, 200)),
    );
    assert_eq!(buf.groups.len(), 3);
    assert_eq!(buf.groups[0].quads.range(), 0..1);
    assert_eq!(buf.groups[1].quads.range(), 1..1);
    assert_eq!(buf.groups[2].quads.range(), 1..2);
    assert_eq!(buf.text_batches.len(), 1);
    assert_eq!(buf.text_batches[0].last_group, 0);
    assert_eq!(buf.backdrops.len(), 1);
    let row = buf.backdrops[0];
    assert_eq!(source(&row), BackdropSource::Capture { barrier: 1 });
    assert_eq!(row.capture(), Some(&ticket.capture));
    let look = row.look.expect("a material's backdrop has a look");
    assert_eq!((look.max_luminance, look.min_luminance), (65504.0, 0.0));
    let effects = buf.batches(PaintTier::Effect);
    assert_eq!(effects.len(), 1);
    assert_eq!(effects[0].last_group, 1);
    assert_eq!(
        (buf.effects.effect()[0], buf.effects.instance()[0].flags),
        (EffectId(0), EffectInstance::MATERIAL)
    );
    let instance = buf.effects.instance()[0];
    assert_eq!((instance.backdrop_idx, instance.opacity), (0, 1.0));
    assert_eq!(instance.rect, rect);
}

/// A held backdrop keeps its result, so it breaks nothing: its effect row joins the open group
/// after the quad before it, and its row is held, under its ticket's region. The quad over it after
/// it opens the next group, as it paints over a draw that replays after quads.
#[test]
fn a_held_backdrop_breaks_nothing() {
    let rect = Rect::new(20.0, 20.0, 40.0, 20.0);
    let ticket = ticket(0, rect, 5.0, false);
    let buf = run(
        |b, _| {
            draw(b, Rect::new(0.0, 0.0, 90.0, 90.0));
            ticketed(b, rect, ticket, 1.0);
            draw(b, Rect::new(30.0, 30.0, 5.0, 5.0));
        },
        &params(1.0, UVec2::new(200, 200)),
    );
    assert_eq!(buf.groups.len(), 2);
    assert_eq!(buf.groups[0].quads.range(), 0..1);
    assert_eq!(buf.batches(PaintTier::Effect)[0].last_group, 0);
    assert_eq!(buf.groups[1].quads.range(), 1..2);
    let row = buf.backdrops[0];
    assert_eq!(source(&row), BackdropSource::Held);
    assert_eq!(row.capture().map(|c| c.region), Some(ticket.capture.region));
}

/// A quad after a backdrop covering one before it leaves it alone: the backdrop's capture copies
/// what it painted. σ 4 at scale 1 reaches 16 px, so the backdrop's footprint takes in the first
/// quad.
#[test]
fn no_prune_crosses_a_barrier() {
    let buf = run(
        |b, _| {
            draw_marked(b, Rect::new(0.0, 0.0, 100.0, 100.0));
            backdrop(b, Rect::new(105.0, 105.0, 10.0, 10.0), 4.0, 1.0);
            draw_marked(b, Rect::new(0.0, 0.0, 100.0, 100.0));
        },
        &params(1.0, UVec2::new(200, 200)),
    );
    assert_eq!(survivor_calls(&buf), [0, 2], "the first quad and the last");
    assert_eq!(buf.effects.len(), 1, "the backdrop");
}

/// A paint animation's fade scales the backdrop's opacity, which fades the backdrop and the fill
/// together; a backdrop faded to nothing draws nothing and cuts nothing.
#[test]
fn a_fade_scales_the_backdrops_opacity() {
    let buf = run(
        |b, _| {
            backdrop(b, Rect::new(0.0, 0.0, 10.0, 10.0), 4.0, 0.5);
            backdrop(b, Rect::new(0.0, 0.0, 10.0, 10.0), 4.0, 0.0);
            // Below `EPS`, as a paint animation's alpha is read: shows nothing, lists nothing.
            backdrop(b, Rect::new(0.0, 0.0, 10.0, 10.0), 4.0, 1e-6);
        },
        &params(1.0, UVec2::new(100, 100)),
    );
    assert_eq!(buf.backdrops.len(), 1);
    assert_eq!(buf.effects.len(), 1);
    assert_eq!(buf.effects.instance()[0].opacity, 0.5);
    let look = buf.backdrops[0]
        .look
        .expect("a material's backdrop has a look");
    assert_eq!(
        RgbaF32::from(look.fill.color).a,
        0.125,
        "the fill's quarter alpha, halved"
    );
}

/// An opaque quad over the whole surface after a backdrop hides the backdrop: the clear fold drops
/// its effect row and keeps its backdrop row, hidden, so it neither captures nor keeps a result
/// that goes stale under the cover.
#[test]
fn the_clear_fold_hides_the_backdrops_before_it() {
    let buf = run(
        |b, _| {
            backdrop(b, Rect::new(0.0, 0.0, 10.0, 10.0), 4.0, 1.0);
            draw(b, Rect::new(0.0, 0.0, 100.0, 100.0));
        },
        &params(1.0, UVec2::new(100, 100)),
    );
    let sources: Vec<BackdropSource> = buf.backdrops.iter().map(source).collect();
    assert_eq!(sources, [BackdropSource::Hidden]);
    assert!(buf.quads.is_empty());
    assert!(buf.effects.is_empty());
}

/// A backdrop with nothing composed before it stands at barrier 0, which the backend reads as a
/// backdrop of the clear: also after a quad over the whole surface folds into the clear. A backdrop
/// right after it has the first backdrop's group before it, so it captures what the first painted.
#[test]
fn a_backdrop_first_in_the_frame_has_nothing_before_it() {
    let buf = run(
        |b, _| {
            draw(b, Rect::new(0.0, 0.0, 100.0, 100.0));
            backdrop(b, Rect::new(10.0, 10.0, 20.0, 20.0), 4.0, 1.0);
            backdrop(b, Rect::new(32.0, 10.0, 20.0, 20.0), 4.0, 1.0);
        },
        &params(1.0, UVec2::new(100, 100)),
    );
    assert!(
        buf.clear_override.is_some(),
        "the cover folds into the clear"
    );
    assert_eq!(barriers(&buf), [0, 1]);
}

/// A backdrop whose footprint misses everything painted since the last break captures there: at 0,
/// the clear, when it misses everything; at the last backdrop's barrier when it misses that
/// backdrop and what followed. One that takes in the last backdrop breaks anew. σ 4 at scale 1
/// reaches 16 px.
#[test]
fn a_backdrop_shares_the_last_break_when_nothing_since_touches_it() {
    let buf = run(
        |b, _| {
            draw(b, Rect::new(0.0, 0.0, 40.0, 40.0));
            backdrop(b, Rect::new(100.0, 100.0, 20.0, 20.0), 4.0, 1.0);
            backdrop(b, Rect::new(10.0, 10.0, 20.0, 20.0), 4.0, 1.0);
            backdrop(b, Rect::new(10.0, 150.0, 20.0, 20.0), 4.0, 1.0);
            backdrop(b, Rect::new(40.0, 150.0, 20.0, 20.0), 4.0, 1.0);
        },
        &params(1.0, UVec2::new(200, 200)),
    );
    assert_eq!(
        barriers(&buf),
        [0, 1, 1, 2],
        "apart from the quad, on it, apart, beside the last"
    );
}

/// A material over the ground, while transparency is reduced, lists a row of the ground over its
/// own rect: it holds no capture, breaks no group, and its effect row reads that row through its
/// material's effect, between the quads around it in one group.
#[test]
fn a_material_over_the_ground_breaks_nothing() {
    let rect = Rect::new(20.0, 20.0, 40.0, 20.0);
    let buf = run(
        |b, _| {
            draw(b, Rect::new(0.0, 0.0, 90.0, 90.0));
            b.draw(
                DrawBackdropPayload {
                    coverage: Coverage::Inside,
                    rect,
                    corners: Corners::ZERO,
                    spin: None,
                    fill: BrushSource::Solid(RgbaF32::new(1.0, 1.0, 1.0, 0.25).to_f16()).gpu_fill(),
                    fill_axis: FillAxis::ZERO,
                    stroke: LoweredBorder::NONE,
                    material: LoweredMaterial::fixture(4.0),
                    opacity: 1.0,
                    read: BackdropDraw::Ground,
                },
                1.0,
            );
        },
        &params(1.0, UVec2::new(200, 200)),
    );
    assert_eq!(buf.groups.len(), 1);
    assert_eq!(buf.backdrops.len(), 1);
    let row = buf.backdrops[0];
    assert_eq!(row.read, BackdropRead::Ground);
    assert_eq!(row.capture(), None);
    assert_eq!(row.reads, rect);
    assert_eq!(
        (buf.effects.effect()[0], buf.effects.instance()[0].flags),
        (EffectId(0), EffectInstance::MATERIAL)
    );
    assert_eq!(buf.effects.instance()[0].backdrop_idx, 0);
}
