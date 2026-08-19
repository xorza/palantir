//! The arithmetic a composed frame is cut with: how a curve is subdivided, how
//! a join is chosen, and how a logical rectangle lands on physical pixels.

use crate::display::Display;
use crate::primitives::approx::EPS;
use crate::primitives::span::Span;
use crate::primitives::{num::F32Ext, rect::Rect, transform::TranslateScale, urect::URect};
use crate::renderer::render_buffer::curve::{
    CURVE_KIND_JOIN_BEVEL, CURVE_KIND_JOIN_MITER, CURVE_KIND_JOIN_ROUND, CurveInstance,
    SEGMENTS_PER_INSTANCE,
};
use crate::renderer::render_buffer::{MAX_ROUNDED_CLIP_DEPTH, RenderBuffer};
use crate::shape::stroke_bounds::{HALF_FRINGE, MITER_LIMIT, stroked_bbox};
use crate::shape::style::{LineCap, LineJoin};
use glam::{UVec2, Vec2};

/// Upper bound on sub-instances per curve. Long, fast-curving strokes
/// (think a 4k-px-long swooping bezier at 200% zoom) hit this cap;
/// beyond it the chord error rises but stays well under a pixel for
/// any realistic UI workload. Cap is a sanity belt — far above the
/// 1–4 sub-instance steady state.
const MAX_SUB_INSTANCES: u32 = 256;

/// Target chord length for GPU-stroke subdivision, physical px. The
/// shader bakes `SEGMENTS_PER_INSTANCE` chords per instance; the
/// composer sizes the instance count so each chord lands near this
/// length — short enough that the 0.5 px AA fringe fully covers any
/// sub-pixel kink between chords. Shared by the cubic (control-polygon
/// length bound) and arc (exact `r·|sweep|` length) paths.
const TARGET_CHORD_PX: f32 = 1.5;

/// Sub-instance count for a GPU stroke of on-screen length `len_px`:
/// enough `SEGMENTS_PER_INSTANCE`-chord instances that each chord
/// lands near [`TARGET_CHORD_PX`], clamped to [`MAX_SUB_INSTANCES`].
#[inline]
pub(super) fn sub_instance_count(len_px: f32) -> u32 {
    let total_segments = (len_px / TARGET_CHORD_PX).ceil().max(1.0) as u32;
    total_segments
        .div_ceil(SEGMENTS_PER_INSTANCE)
        .clamp(1, MAX_SUB_INSTANCES)
}

/// Tile `t ∈ [0, 1]` into `n` contiguous ranges (the last ending at
/// exactly `1.0`, so the shader's trailing-cap test fires) and push
/// one instance per range; `proto` supplies every other lane.
pub(super) fn push_sub_instances(out: &mut RenderBuffer, n: u32, proto: CurveInstance) {
    let inv_n = 1.0 / n as f32;
    for i in 0..n {
        let t1 = if i + 1 == n {
            1.0
        } else {
            (i + 1) as f32 * inv_n
        };
        out.curves.push(CurveInstance {
            t0: i as f32 * inv_n,
            t1,
            ..proto
        });
    }
}

/// Squared distance below which two consecutive transformed polyline
/// points count as coincident and the latter is dropped — a
/// zero-length segment has no direction (`normalize` would NaN the
/// joint planes), so it must contribute no geometry, and its color
/// drops with it.
pub(super) const POLYLINE_COINCIDENT_EPS_SQ: f32 = 1e-12;

/// Chrome kind for the joint between two polyline segments with unit
/// directions `d_a` (into the joint) and `d_b` (out of it). `Miter`
/// downgrades to bevel past [`MITER_LIMIT`] — the SVG convention; an
/// antiparallel fold (180°, bisector undefined) renders round — the
/// only join whose shape is well-defined there.
pub(super) fn polyline_join_kind(d_a: Vec2, d_b: Vec2, join: LineJoin) -> u32 {
    let sum = d_a + d_b;
    let len_sq = sum.length_squared();
    if len_sq < 1e-6 {
        return CURVE_KIND_JOIN_ROUND;
    }
    match join {
        LineJoin::Round => CURVE_KIND_JOIN_ROUND,
        LineJoin::Bevel => CURVE_KIND_JOIN_BEVEL,
        LineJoin::Miter => {
            // |d_a + d_b| = 2·cos(half turn angle) for unit inputs.
            let cos_half = 0.5 * len_sq.sqrt();
            if cos_half < 1.0 / MITER_LIMIT {
                CURVE_KIND_JOIN_BEVEL
            } else {
                CURVE_KIND_JOIN_MITER
            }
        }
    }
}

/// Max perpendicular distance (physical px) of a cubic's inner control
/// points from the chord line for the curve to count as flat. The
/// curve deviates at most `3/4 · max(d1, d2)` from the chord, so at
/// this threshold it sits within ~0.075 px of a straight line —
/// invisible under the 0.5 px AA fringe at any chord density.
const FLAT_EPS_PX: f32 = 0.1;

/// True when the cubic's trace is visually indistinguishable from the
/// straight segment `p0 → p3` (see [`FLAT_EPS_PX`]). Both inner CPs
/// must sit within the threshold of the *infinite* chord line; a
/// degenerate chord (closed curve) is never flat.
#[inline]
pub(super) fn cubic_is_flat(p0: Vec2, p1: Vec2, p2: Vec2, p3: Vec2) -> bool {
    let chord = p3 - p0;
    let len = chord.length();
    if len <= FLAT_EPS_PX {
        return false;
    }
    let d1 = chord.perp_dot(p1 - p0).abs();
    let d2 = chord.perp_dot(p2 - p0).abs();
    d1.max(d2) <= FLAT_EPS_PX * len
}

/// Additive step on the text-scale ladder. Same step in *scale units*
/// across the range, so the step in *percent of current size* shrinks
/// as zoom grows (0.005/4 ≈ 0.125% at 4×, 0.005/1 = 0.5% at 1×, 0.005/0.5
/// = 1% at 0.5×). The user-perceptual case for this layout: at high
/// zoom every percent of size change is visible, so we want fine steps;
/// at low zoom text is small and crispness stepping doesn't matter, so
/// coarse steps + fewer atlas keys is the right trade.
///
/// **Geometric note.** Measurement uses the unscaled `font_size_px`
/// (text layout shaping) — only the paint-time scale is snapped. At a
/// non-rung zoom level the rendered glyph block is up to `STEP/2`
/// wider/narrower than the layout-space rect it nominally fills. The
/// extra width is clipped at `TextDrawRow.bounds`, and the cascade
/// inflates text damage rects by the same fraction so a rung-jump
/// between consecutive frames repaints all affected pixels (see
/// `scene::shapes::record::text_paint_bbox_local`).
///
/// Sourced from [`crate::text::TEXT_SCALE_STEP`] so the cascade's
/// inflation and the composer's snap stay locked in step.
const TEXT_SCALE_STEP: f32 = crate::text::TEXT_SCALE_STEP;

/// Snap the ancestor-transform component of a text run's scale to the
/// additive 0.5% ladder. Identity is preserved exactly so non-zoom UIs
/// stay on the trivial path. See call-site comment in `DrawText` for
/// rationale.
pub(super) fn snap_text_scale(s: f32) -> f32 {
    if (s - 1.0).abs() < EPS {
        return 1.0;
    }
    (s / TEXT_SCALE_STEP).fast_round() * TEXT_SCALE_STEP
}

/// The pixels a physical-px AABB covers, held inside the viewport — the
/// `URect` the GPU can consume.
///
/// Both halves belong to the rectangles rather than here: which pixels a float
/// rect touches is [`URect::covering`], and holding one inside another is
/// [`URect::clamp_to`]. What this adds is the pairing, and the name the
/// composer knows it by.
pub(super) fn urect_from_phys(min: Vec2, max: Vec2, viewport: UVec2) -> URect {
    URect::covering(Rect::from_min_max(min, max)).clamp_to(URect::new(0, 0, viewport.x, viewport.y))
}

pub(super) fn scissor_from_logical(r: Rect, scale: f32, snap: bool, viewport: UVec2) -> URect {
    let phys = r.scaled_by(scale, snap);
    urect_from_phys(phys.min, phys.max(), viewport)
}

/// Value equality of two rounded-mask chains (spans into
/// `out.rounded_clips`). Spans differ across a pop/re-push of an
/// identical clip — the composer pushes a fresh chain per rounded push —
/// but value-equal chains stamp identical masks, so clip-transition
/// decisions must not split on span identity alone.
pub(super) fn chains_equal(out: &RenderBuffer, a: Span, b: Span) -> bool {
    out.rounded_clips[a.range()] == out.rounded_clips[b.range()]
}

#[cold]
#[inline(never)]
pub(super) fn rounded_clip_depth_overflow(depth: u32) -> ! {
    panic!("rounded clip chain depth {depth} exceeds stencil capacity {MAX_ROUNDED_CLIP_DEPTH}");
}

/// Physical-px painted bounds for a stroked shape's owner-local
/// centerline `bbox`. Folds `origin` + the active transform into physical space,
/// applies the shared stroke/cap/join/AA bound once, then clamps to the
/// viewport. Shared by the curve and polyline paths so their cull and
/// overlap bounds cannot drift.
pub(super) fn stroke_bbox_urect(
    xform: TranslateScale,
    bbox: Rect,
    origin: Vec2,
    width_phys: f32,
    cap: LineCap,
    join: Option<LineJoin>,
    display: Display,
) -> URect {
    let world_bbox = xform.apply_rect(Rect {
        min: bbox.min + origin,
        size: bbox.size,
    });
    let centerline_phys = world_bbox.scaled_by(display.scale_factor, false);
    let painted = stroked_bbox(centerline_phys, width_phys, HALF_FRINGE, cap, join);
    urect_from_phys(painted.min, painted.max(), display.physical)
}
