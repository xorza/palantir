//! The arithmetic a composed frame is cut with: how a curve is subdivided, how
//! a join is chosen, and how a logical rectangle lands on physical pixels.

use crate::display::Display;
use crate::primitives::approx::EPS;
use crate::primitives::{num::F32Ext, rect::Rect, translate_scale::TranslateScale, urect::URect};
use crate::renderer::render_buffer::curve::{
    CURVE_KIND_JOIN_BEVEL, CURVE_KIND_JOIN_MITER, CURVE_KIND_JOIN_ROUND, CurveInstance,
    SEGMENTS_PER_INSTANCE,
};
use crate::renderer::render_buffer::{MAX_ROUNDED_CLIP_DEPTH, RenderBuffer};
use crate::shape::stroke_bounds::{HALF_FRINGE, MITER_LIMIT, stroked_bbox};
use crate::shape::style::{LineCap, LineJoin};
use crate::text::TEXT_SCALE_STEP;
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

/// Physical pixels per owner-local unit under `xform`.
///
/// Named because six draw paths need it and reached for it two ways —
/// through the stack's `scale()` and through a `current()` already in
/// hand — which is one spelling too many for a number every one of them
/// multiplies a stroke width or a radius by.
#[inline]
pub(super) fn phys_scale(xform: TranslateScale, display_scale: f32) -> f32 {
    xform.scale * display_scale
}

/// The map from owner-local logical px to physical px: fold in the
/// owner's origin, place by the active transform, scale by the display
/// factor.
///
/// A closure rather than a per-point call, because every caller applies
/// it to a run of points and the transform read belongs outside that
/// loop.
#[inline]
pub(super) fn phys_point_map(
    xform: TranslateScale,
    origin: Vec2,
    display_scale: f32,
) -> impl Fn(Vec2) -> Vec2 {
    move |q| xform.apply_point(q + origin) * display_scale
}

/// [`phys_point_map`]'s rect: an owner-local bbox in physical px.
///
/// Unsnapped — the tiers that fold a bbox this way (mesh, and the stroked
/// pair through [`stroke_bbox_urect`]) all place sub-pixel geometry and
/// let their shaders resolve the fringe.
#[inline]
pub(super) fn phys_bbox(
    xform: TranslateScale,
    bbox: Rect,
    origin: Vec2,
    display_scale: f32,
) -> Rect {
    xform
        .apply_rect(Rect {
            min: bbox.min + origin,
            size: bbox.size,
        })
        .scaled_by(display_scale, false)
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
    let centerline_phys = phys_bbox(xform, bbox, origin, display.scale_factor);
    let painted = stroked_bbox(centerline_phys, width_phys, HALF_FRINGE, cap, join);
    urect_from_phys(painted.min, painted.max(), display.physical)
}
