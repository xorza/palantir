//! Curve-pipeline wire constants and per-instance GPU data.

use crate::primitives::packed::fill_kind::FillKind;
use crate::primitives::paint::color::rgba_f16::RgbaF16;
use crate::primitives::paint::lut_row::LutRow;
use crate::renderer::render_buffer::curve_caps::CurveCaps;
use crate::renderer::render_buffer::curve_kind::CurveKind;
use glam::Vec2;

/// Chord-subdivisions per curve sub-instance. The shader expands one
/// instance into this many quads (= 2× this many triangles = 6× this
/// many indices), and takes the value as a substituted constant. Lives here, next to
/// [`CurveInstance`], because it's part of the composer↔backend wire
/// contract: the composer's sub-instance math and the backend's
/// per-instance vertex count both derive from it.
pub(crate) const SEGMENTS_PER_INSTANCE: u32 = 16;

/// Per-curve-sub-instance GPU state, uploaded to a
/// `step_mode: Instance` vertex buffer. For the strip kinds the
/// shader evaluates the stroke's parametric basis (picked by `kind`)
/// at parameter `t = mix(t0, t1, segment / SEGMENTS_PER_INSTANCE)`
/// for `segment ∈ [0, SEGMENTS_PER_INSTANCE]`, derives the tangent's
/// perpendicular, and offsets by ±(width/2 + AA fringe) to build the
/// stroked strip. All geometry lanes are pre-transformed to
/// physical-px; `width` is also physical px. Colors are linear-RGBA
/// straight-alpha; the fragment shader premultiplies at output.
///
/// Lane meaning by `kind`:
/// - [`CurveKind::CUBIC`] — `p0..p3` are the cubic control points.
/// - [`CurveKind::ARC`] — `p0` = center, `p1.x` = radius,
///   `p2 = (a0, a1)` start/end angle in radians (screen convention:
///   0 = +x, y-down ⇒ increasing = clockwise); `p1.y`/`p3` unused.
///   The angle at `t` is `mix(a0, a1, t)` — exact circle, no cubic
///   approximation error, and ramp `t` tracks the sweep linearly.
/// - [`CurveKind::SEGMENT`] — `p0`/`p3` are the segment endpoints;
///   `p1`/`p2` carry the pre-oriented bisector clip-plane normals
///   for the start/end joint (zero = cap end, no clip; "keep" is
///   `dot(x - endpoint, n) <= 0`). Joint ends are butt-faced and
///   fragment-clipped at those planes — the composer hands adjacent
///   segments exact negations of the same sum, so strips partition
///   their concave overlap exactly (no double blend on translucent
///   strokes), and the convex wedge is filled by a join-chrome
///   instance.
/// - `CurveKind::JOIN_*` — `p0` = joint point; `p1 = -d_a`,
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
    /// Stroke colour at `t = 0`. Under a ramp `fill_kind` it multiplies
    /// the colour the shader samples from the LUT row.
    pub(crate) color0: RgbaF16,
    /// Stroke colour at `t = 1` — the shader lerps `color0 → color1`
    /// along `t` (straight-alpha, like a polyline's per-point colours).
    /// Equal to `color0` for single-colour strokes.
    pub(crate) color1: RgbaF16,
    /// The stroke's cap and the ends of this instance that are the
    /// stroke's ends. Of those, only the leading sub-instance (`t0 ≈ 0`)
    /// and the trailing one (`t1 ≈ 1`) extend their geometry. A polyline
    /// segment names only the true ends among its own.
    pub(crate) caps: CurveCaps,
    /// Fill kind tag: [`FillKind::SOLID`] or [`FillKind::RAMP`], the two
    /// [`GpuFill::curve`] can make. A curve's `t` is already in [0, 1]
    /// by construction, so no spread rides here. `#[repr(transparent)]`
    /// over `u32`, so the GPU sees the same bytes the `Uint32` vertex
    /// attribute expects.
    ///
    /// [`GpuFill::curve`]: crate::renderer::frontend::payload::gpu_fill::GpuFill::curve
    pub(crate) fill_kind: FillKind,
    /// Atlas row when `fill_kind` is a gradient, else ignored.
    pub(crate) fill_lut_row: LutRow,
    /// How the vertex shader reads the geometry lanes (see struct docs).
    pub(crate) kind: CurveKind,
}
