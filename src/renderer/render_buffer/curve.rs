//! Curve-pipeline wire constants and per-instance GPU data.

use crate::primitives::color::ColorU8;
use crate::primitives::fill_wire::{FillKind, LutRow};
use glam::Vec2;

/// Chord-subdivisions per curve sub-instance. The shader expands one
/// instance into this many quads (= 2× this many triangles = 6× this
/// many indices). Has to stay in lockstep with the constant of the
/// same name in `curve.wgsl` (the curve pipeline stamps this value
/// into the shader source at module creation). Lives here, next to
/// [`CurveInstance`], because it's part of the composer↔backend wire
/// contract: the composer's sub-instance math and the backend's
/// per-instance vertex count both derive from it.
pub(crate) const SEGMENTS_PER_INSTANCE: u32 = 16;

/// Basis tags for [`CurveInstance::kind`]. Pinned against the
/// `KIND_*` constants in `curve.wgsl` — bump together.
pub(crate) const CURVE_KIND_CUBIC: u32 = 0;
pub(crate) const CURVE_KIND_ARC: u32 = 1;
/// Straight polyline segment with bisector-clipped joint ends.
pub(crate) const CURVE_KIND_SEGMENT: u32 = 2;
/// Joint chrome billboards — the three `LineJoin` looks. Contiguous
/// values: the shader derives the fragment metric as
/// `kind - CURVE_KIND_JOIN_ROUND`.
pub(crate) const CURVE_KIND_JOIN_ROUND: u32 = 3;
pub(crate) const CURVE_KIND_JOIN_BEVEL: u32 = 4;
pub(crate) const CURVE_KIND_JOIN_MITER: u32 = 5;

/// Per-curve-sub-instance GPU state, uploaded to a
/// `step_mode: Instance` vertex buffer. For the strip kinds the
/// shader evaluates the stroke's parametric basis (picked by `kind`)
/// at parameter `t = mix(t0, t1, segment / SEGMENTS_PER_INSTANCE)`
/// for `segment ∈ [0, SEGMENTS_PER_INSTANCE]`, derives the tangent's
/// perpendicular, and offsets by ±(width/2 + AA fringe) to build the
/// stroked strip. All geometry lanes are pre-transformed to
/// physical-px; `width` is also physical px. Colors are linear-RGBA
/// straight-alpha (same convention as `MeshVertex.color`); the
/// fragment shader premultiplies at output.
///
/// Lane meaning by `kind`:
/// - [`CURVE_KIND_CUBIC`] — `p0..p3` are the cubic control points.
/// - [`CURVE_KIND_ARC`] — `p0` = center, `p1.x` = radius,
///   `p2 = (a0, a1)` start/end angle in radians (screen convention:
///   0 = +x, y-down ⇒ increasing = clockwise); `p1.y`/`p3` unused.
///   The angle at `t` is `mix(a0, a1, t)` — exact circle, no cubic
///   approximation error, and gradient `t` tracks the sweep linearly.
/// - [`CURVE_KIND_SEGMENT`] — `p0`/`p3` are the segment endpoints;
///   `p1`/`p2` carry the pre-oriented bisector clip-plane normals
///   for the start/end joint (zero = cap end, no clip; "keep" is
///   `dot(x - endpoint, n) <= 0`). Joint ends are butt-faced and
///   fragment-clipped at those planes — the composer hands adjacent
///   segments exact negations of the same sum, so strips partition
///   their concave overlap exactly (no double blend on translucent
///   strokes), and the convex wedge is filled by a join-chrome
///   instance.
/// - `CURVE_KIND_JOIN_*` — `p0` = joint point; `p1 = -d_a`,
///   `p2 = d_b` (unit segment directions into/out of the joint,
///   pre-oriented as the face-plane keep normals). Expands to one
///   billboard quad; the fragment fills the wedge between the two
///   segment end faces with an exact per-kind metric (round: radial;
///   bevel: radial ∧ bevel half-plane; miter: max of the two
///   centerline distances).
#[padding_struct::padding_struct]
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct CurveInstance {
    pub(crate) p0: Vec2,
    pub(crate) p1: Vec2,
    pub(crate) p2: Vec2,
    pub(crate) p3: Vec2,
    /// `[t0, t1]` — the sub-range of the parent curve this instance
    /// covers. The vertex shader subdivides this range into
    /// `SEGMENTS_PER_INSTANCE` chords; one curve emits ⌈N/16⌉
    /// sub-instances where `N` is the adaptive segment count.
    pub(crate) t0: f32,
    pub(crate) t1: f32,
    pub(crate) width: f32,
    /// Stroke colour at `t = 0`. Zeroed when `fill_kind != 0`; the
    /// shader samples the LUT row instead.
    pub(crate) color0: ColorU8,
    /// Stroke colour at `t = 1` — the shader lerps `color0 → color1`
    /// along `t` (straight-alpha, like `PolylineColors::PerPoint`).
    /// Equal to `color0` for single-colour strokes.
    pub(crate) color1: ColorU8,
    /// Cap kind per end, packed: bits 0..8 = start cap, 8..16 = end
    /// cap (0 = Butt, 1 = Square, 2 = Round). Only the leading
    /// sub-instance (`t0 ≈ 0`) and trailing sub-instance (`t1 ≈ 1`)
    /// actually extend their geometry; interior sub-instances see
    /// this lane and skip cap extension. Polyline segments carry the
    /// user cap on true ends and Butt on joint ends.
    pub(crate) cap: u32,
    /// Brush kind tag. Low byte 0 = solid, 1 = linear. Spread mode
    /// would ride in bits 8..16 like the quad pipeline, but a curve's
    /// `t` is already clamped to [0, 1] by construction, so spread is
    /// a no-op here. `#[repr(transparent)]` over `u32`, so the GPU
    /// sees the same bytes the `Uint32` vertex attribute expects.
    pub(crate) fill_kind: FillKind,
    /// Atlas row when `fill_kind` is a gradient, else ignored.
    pub(crate) fill_lut_row: LutRow,
    /// Basis tag — one of the `CURVE_KIND_*` constants. Selects how
    /// the vertex shader interprets the geometry lanes (see struct
    /// docs).
    pub(crate) kind: u32,
}

/// Pack per-end cap kinds into the [`CurveInstance::cap`] lane.
#[inline]
pub(crate) fn cap_lanes(start: u32, end: u32) -> u32 {
    start | (end << 8)
}
