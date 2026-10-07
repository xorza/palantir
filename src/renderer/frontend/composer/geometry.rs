//! Arithmetic a composed frame is cut with: curve subdivision, join choice, and
//! how a logical rect lands on physical pixels.

use crate::display::Display;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::geometry::translate_scale::TranslateScale;
use crate::primitives::geometry::urect::URect;
use crate::primitives::math::domain::EPS;
use crate::primitives::math::num::F32Px;
use crate::primitives::paint::antialias::AA_HALF_WIDTH;
use crate::renderer::render_buffer::MAX_ROUNDED_CLIP_DEPTH;
use crate::renderer::render_buffer::curve::SEGMENTS_PER_INSTANCE;
use crate::renderer::render_buffer::curve_kind::CurveKind;
use crate::shape::stroke_bounds::{self, MITER_LIMIT};
use crate::shape::style::{LineCap, LineJoin};
use crate::text::TEXT_SCALE_STEP;
use glam::{UVec2, Vec2};

/// Upper bound on sub-instances per curve, a sanity belt far above the 1-4
/// steady state; past it the chord error rises but stays under a pixel.
const MAX_SUB_INSTANCES: u32 = 256;

/// Target chord length for GPU-stroke subdivision, physical px: short enough
/// that the 0.5 px AA fringe covers any kink between chords. Shared by the
/// cubic (control-polygon bound) and arc (`r*|sweep|`) paths.
const TARGET_CHORD_PX: f32 = 1.5;

/// Sub-instance count for a GPU stroke of on-screen length `len_px`, so each
/// chord lands near [`TARGET_CHORD_PX`]; clamped to [`MAX_SUB_INSTANCES`].
#[inline]
#[expect(
    clippy::cast_sign_loss,
    reason = "the segment count is held at 1 or more before the cast"
)]
pub(super) fn sub_instance_count(len_px: f32) -> u32 {
    let total_segments = (len_px / TARGET_CHORD_PX).ceil().max(1.0) as u32;
    total_segments
        .div_ceil(SEGMENTS_PER_INSTANCE)
        .clamp(1, MAX_SUB_INSTANCES)
}

/// Squared distance below which consecutive transformed polyline points are
/// coincident and the latter is dropped: a zero-length segment has no
/// direction (`normalize` would NaN the joint planes).
pub(super) const POLYLINE_COINCIDENT_EPS_SQ: f32 = 1e-12;

/// Join kind between two polyline segments with unit directions `d_a` (into
/// the joint) and `d_b` (out). `Miter` downgrades to bevel past
/// [`MITER_LIMIT`] (the SVG convention); an antiparallel fold renders round,
/// the only well-defined join there.
pub(super) fn polyline_join_kind(d_a: Vec2, d_b: Vec2, join: LineJoin) -> CurveKind {
    let sum = d_a + d_b;
    let len_sq = sum.length_squared();
    if len_sq < 1e-6 {
        return CurveKind::JOIN_ROUND;
    }
    match join {
        LineJoin::Round => CurveKind::JOIN_ROUND,
        LineJoin::Bevel => CurveKind::JOIN_BEVEL,
        LineJoin::Miter => {
            let cos_half = 0.5 * len_sq.sqrt();
            if cos_half < 1.0 / MITER_LIMIT {
                CurveKind::JOIN_BEVEL
            } else {
                CurveKind::JOIN_MITER
            }
        }
    }
}

/// Max perpendicular distance (physical px) of a cubic's inner control points
/// from the chord for the curve to count as flat. The curve deviates at most
/// `3/4 * max(d1, d2)`, so ~0.075 px: invisible under the AA fringe.
const FLAT_EPS_PX: f32 = 0.1;

/// True when the cubic is indistinguishable from the straight segment
/// `p0 -> p3` ([`FLAT_EPS_PX`]): both inner CPs within the threshold of the
/// *infinite* chord line. A degenerate chord (closed curve) is never flat.
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
/// additive 0.5% ladder; identity is exact so non-zoom UIs stay on the trivial
/// path.
pub(super) fn snap_text_scale(s: f32) -> f32 {
    if (s - 1.0).abs() < EPS {
        return 1.0;
    }
    (s / TEXT_SCALE_STEP).fast_round() * TEXT_SCALE_STEP
}

/// The pixels a physical-px AABB covers, held inside the viewport: the `URect`
/// the GPU consumes. Pairs [`URect::covering`] with [`URect::clamp_to`].
pub(super) fn urect_from_phys(min: Vec2, max: Vec2, viewport: UVec2) -> URect {
    URect::covering(Rect::from_min_max(min, max)).clamp_to(URect::new(0, 0, viewport.x, viewport.y))
}

/// Physical pixels per owner-local unit under `xform`.
#[inline]
pub(super) const fn phys_scale(xform: TranslateScale, display_scale: f32) -> f32 {
    xform.scale * display_scale
}

/// The map from owner-local logical px to physical px: owner origin, active
/// transform, display factor. A closure so the transform read sits outside
/// callers' per-point loops.
#[inline]
pub(super) fn phys_point_map(
    xform: TranslateScale,
    origin: Vec2,
    display_scale: f32,
) -> impl Fn(Vec2) -> Vec2 {
    move |q| xform.apply_point(q + origin) * display_scale
}

/// [`phys_point_map`]'s rect: an owner-local bbox in physical px. Unsnapped,
/// since mesh and stroked tiers place sub-pixel geometry and let shaders
/// resolve the fringe (see [`StrokeBbox::urect`]).
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

/// One stroked shape's owner-local centerline and the style laid over it.
/// Named fields because the curve and polyline paths differ in two and agree
/// on the rest.
#[derive(Clone, Copy, Debug)]
pub(super) struct StrokeBbox {
    pub(super) xform: TranslateScale,
    /// Owner-local centerline bounds, before `origin` and `xform`.
    pub(super) bbox: Rect,
    pub(super) origin: Vec2,
    pub(super) width_phys: f32,
    pub(super) cap: LineCap,
    /// `None` for a single-segment stroke, which has no joint.
    pub(super) join: Option<LineJoin>,
    pub(super) display: Display,
}

impl StrokeBbox {
    /// Physical-px painted bounds: folds in `origin` and the active transform,
    /// applies the shared stroke/cap/join/AA bound, clamps to the viewport. Shared
    /// by curve and polyline paths so their cull and overlap bounds agree.
    pub(super) fn urect(self) -> URect {
        let Self {
            xform,
            bbox,
            origin,
            width_phys,
            cap,
            join,
            display,
        } = self;
        let centerline_phys = phys_bbox(xform, bbox, origin, display.scale_factor());
        let painted = stroke_bounds::bbox(centerline_phys, width_phys, AA_HALF_WIDTH, cap, join);
        urect_from_phys(painted.min, painted.max(), display.physical)
    }
}
