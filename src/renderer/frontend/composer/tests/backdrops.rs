//! Where a backdrop cuts the frame, and what its row records.

use crate::cascade::plane_backdrop::PlaneBackdrop;
use crate::common::span::Span;
use crate::internals::paint_capture::PaintCapture;
use crate::primitives::geometry::corners::Corners;
use crate::primitives::geometry::coverage::Coverage;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::geometry::urect::URect;
use crate::primitives::identity::backdrop_key::BackdropKey;
use crate::primitives::identity::effect_id::EffectId;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::paint::border::Border;
use crate::primitives::paint::color::RgbaF32;
use crate::renderer::backdrop_captures::BackdropTicket;
use crate::renderer::frontend::composer::tests::support::{
    draw, draw_marked, effect, params, run, survivor_calls, text,
};
use crate::renderer::frontend::paint_sink::PaintSink;
use crate::renderer::frontend::payload::brush_source::BrushSource;
use crate::renderer::frontend::payload::draw_effect_payload::{
    DrawEffectPayload, EffectCaptureDraw, EffectGeometry,
};
use crate::renderer::frontend::payload::draw_quad_payload::{DrawQuadPayload, TriangleQuad};
use crate::renderer::render_buffer::RenderBuffer;
use crate::renderer::render_buffer::backdrop_row::{
    BackdropCapture, BackdropRead, BackdropRow, BackdropSource,
};
use crate::renderer::render_buffer::effect_draw_row::EffectInstance;
use crate::renderer::render_buffer::paint_tier::PaintTier;
use crate::shape::geometry::box_geometry::PlacedBox;
use crate::shape::geometry::triangle_geometry::PlacedTriangle;
use crate::shape::paint::blur_plan::BlurPlan;
use crate::shape::paint::capture_params::CaptureReach;
use crate::shape::paint::lowered_border::LoweredBorder;
use crate::shape::paint::lowered_material::LoweredMaterial;
use glam::{UVec2, Vec2};

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

/// A material of σ 4 over `geometry` reading `read`, under a white fill at α 0.25 and `border`.
fn material(
    geometry: EffectGeometry,
    read: EffectCaptureDraw,
    border: LoweredBorder,
) -> DrawEffectPayload {
    DrawEffectPayload::material(
        LoweredMaterial::fixture(4.0),
        read,
        geometry,
        BrushSource::Solid(RgbaF32::new(1.0, 1.0, 1.0, 0.25).to_f16()),
        border,
    )
}

/// The box at `rect`, square-cornered, covering its inside.
const fn sharp(rect: Rect) -> EffectGeometry {
    EffectGeometry::Box {
        coverage: Coverage::Inside,
        placed: PlacedBox {
            rect,
            corners: Corners::ZERO,
        },
    }
}

/// A backdrop at `rect` under `ticket`, faded by `alpha`; the ticket holds its plan, so its own σ
/// is any.
fn ticketed(buf: &mut PaintCapture, rect: Rect, ticket: BackdropTicket, alpha: f32) {
    let read = EffectCaptureDraw::Capture {
        ticket,
        reach: CaptureReach::Px(0.0),
    };
    buf.draw(material(sharp(rect), read, LoweredBorder::NONE), alpha);
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
/// material's effect, between the quads around it in one group. Its instance holds its shape as
/// the template takes it: the 40 × 20 rect's corners, its inradius half the short side, 10, and its
/// border's width, 3, which the template keeps out of its interior.
#[test]
fn a_material_over_the_ground_breaks_nothing() {
    let rect = Rect::new(20.0, 20.0, 40.0, 20.0);
    let buf = run(
        |b, _| {
            draw(b, Rect::new(0.0, 0.0, 90.0, 90.0));
            b.draw(
                material(
                    sharp(rect),
                    EffectCaptureDraw::Ground,
                    LoweredBorder::lower(Border::new(RgbaF32::WHITE, 3.0)),
                ),
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
    let instance = buf.effects.instance()[0];
    assert_eq!(
        instance.points,
        [
            Vec2::ZERO,
            Vec2::new(40.0, 0.0),
            Vec2::new(40.0, 20.0),
            Vec2::new(0.0, 20.0)
        ]
    );
    assert_eq!(instance.inradius, 10.0);
    assert_eq!(instance.border_width, 3.0);
}

/// An app's effect holds its shape as the template takes it, its rect's corners at scale 2, its
/// inradius half the short side, and no border: (5, 5, 30 × 10) is (10, 10, 60 × 20).
#[test]
fn an_effect_holds_its_shape_for_the_template() {
    let buf = run(
        |b, _| effect(b, Rect::new(5.0, 5.0, 30.0, 10.0)),
        &params(2.0, UVec2::new(200, 200)),
    );
    let instance = buf.effects.instance()[0];
    assert_eq!(instance.rect, Rect::new(10.0, 10.0, 60.0, 20.0));
    assert_eq!(
        instance.points,
        [
            Vec2::ZERO,
            Vec2::new(60.0, 0.0),
            Vec2::new(60.0, 20.0),
            Vec2::new(0.0, 20.0)
        ]
    );
    assert_eq!(instance.inradius, 10.0);
    assert_eq!(instance.border_width, 0.0);
}

/// A triangle's material packs its shape as a triangle quad does, and its instance flags the
/// triangle, so the effect stage builds three corners: (5, 5), (25, 5), (5, 15) with radius 1 at
/// scale 2 cover (8, 8) to (52, 32).
#[test]
fn a_triangles_material_packs_as_its_triangle() {
    let points = [
        Vec2::new(5.0, 5.0),
        Vec2::new(25.0, 5.0),
        Vec2::new(5.0, 15.0),
    ];
    let buf = run(
        |b, _| {
            b.draw(
                material(
                    EffectGeometry::Triangle(PlacedTriangle {
                        centers: points,
                        radius: 1.0,
                    }),
                    EffectCaptureDraw::Ground,
                    LoweredBorder::NONE,
                ),
                1.0,
            );
            b.draw(
                DrawQuadPayload::triangle(TriangleQuad {
                    points,
                    radius: 1.0,
                    fill: BrushSource::Solid(RgbaF32::WHITE.to_f16()),
                    border: LoweredBorder::NONE,
                }),
                1.0,
            );
        },
        &params(2.0, UVec2::new(200, 200)),
    );
    let instance = buf.effects.instance()[0];
    let quad = buf.quads[0];
    assert_eq!(
        instance.flags,
        EffectInstance::MATERIAL | EffectInstance::TRIANGLE
    );
    assert_eq!(instance.corner_count(), 3);
    assert_eq!(
        instance.rect,
        Rect::from_min_max(Vec2::new(8.0, 8.0), Vec2::new(52.0, 32.0))
    );
    assert_eq!(instance.rect, quad.rect);
    assert_eq!(instance.points, quad.points);
    assert_eq!(instance.inradius, quad.inradius);
    assert_eq!(instance.corners, quad.corners.to_array());
}

/// A material and an app's backdrop effect over the same rect, each reaching nothing past it, land
/// through one path: their instances differ in the material flag and the backdrop they read alone,
/// and their rows in the look the material's stage holds; both read the rect itself, inside their
/// footprints.
#[test]
fn a_material_and_a_backdrop_effect_share_one_path() {
    let rect = Rect::new(20.0, 20.0, 40.0, 20.0);
    let read = |i| EffectCaptureDraw::Capture {
        ticket: ticket(i, rect, 4.0, true),
        reach: CaptureReach::Px(0.0),
    };
    let buf = run(
        |b, _| {
            b.draw(material(sharp(rect), read(0), LoweredBorder::NONE), 1.0);
            b.draw(
                DrawEffectPayload {
                    geometry: sharp(rect),
                    spin: None,
                    effect: EffectId(0),
                    params: Span::default(),
                    images: Span::default(),
                    backdrop: Some(read(1)),
                    stage: None,
                    opacity: 1.0,
                    time: 0.0,
                },
                1.0,
            );
        },
        &params(1.0, UVec2::new(200, 200)),
    );
    let [worn, painted] = [buf.effects.instance()[0], buf.effects.instance()[1]];
    assert_eq!(worn.flags, EffectInstance::MATERIAL);
    assert_eq!(
        painted,
        EffectInstance {
            flags: 0,
            backdrop_idx: painted.backdrop_idx,
            ..worn
        }
    );
    assert_ne!(worn.backdrop_idx, painted.backdrop_idx);
    let [worn, painted] = [
        buf.backdrops[worn.backdrop_idx as usize],
        buf.backdrops[painted.backdrop_idx as usize],
    ];
    assert!(worn.look.is_some(), "a material's row holds its look");
    assert_eq!(painted.look, None);
    assert_eq!(worn.reads, rect);
    assert_eq!(painted.reads, rect);
}
