//! Curve-pipeline wire constants and per-instance GPU data.

use crate::primitives::packed::fill_kind::FillKind;
use crate::primitives::paint::color::rgba_f16::RgbaF16;
use crate::primitives::paint::lut_row::LutRow;
use crate::renderer::render_buffer::curve_caps::CurveCaps;
use crate::renderer::render_buffer::curve_kind::CurveKind;
use glam::Vec2;

/// Chord subdivisions per curve sub-instance; the composer and backend both derive from it.
pub(crate) const SEGMENTS_PER_INSTANCE: u32 = 16;

/// Per-curve-sub-instance GPU state. The shader evaluates the basis picked by
/// `kind` at `t = mix(t0, t1, segment / SEGMENTS_PER_INSTANCE)` and offsets by
/// ±(width/2 + AA fringe). Geometry and `width` are physical px.
///
/// Lanes by `kind`:
/// - [`CurveKind::CUBIC`]: `p0..p3` are the control points.
/// - [`CurveKind::ARC`]: `p0` = centre, `p1.x` = radius, `p2 = (a0, a1)`
///   start/end radians (0 = +x, y-down).
/// - [`CurveKind::SEGMENT`]: `p0`/`p3` are the endpoints; `p1`/`p2` are
///   pre-oriented bisector clip normals (zero = cap end). Adjacent segments
///   get exact negations so strips partition their overlap with no double blend.
/// - `CurveKind::JOIN_*`: `p0` = joint, `p1 = -d_a`, `p2 = d_b` (unit
///   directions); one billboard quad filling the wedge.
#[padding_struct::padding_struct]
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct CurveInstance {
    pub(crate) p0: Vec2,
    pub(crate) p1: Vec2,
    pub(crate) p2: Vec2,
    pub(crate) p3: Vec2,
    /// `[t0, t1]`: the sub-range of the parent curve this instance covers.
    pub(crate) t0: f32,
    pub(crate) t1: f32,
    pub(crate) width: f32,
    /// Stroke colour at `t = 0`; under a ramp it multiplies the LUT colour.
    pub(crate) color0: RgbaF16,
    /// Stroke colour at `t = 1`, lerped along `t`.
    pub(crate) color1: RgbaF16,
    /// The stroke's cap and which ends of this instance are the stroke's ends.
    pub(crate) caps: CurveCaps,
    /// [`FillKind::SOLID`] or [`FillKind::RAMP`]; `t` is already in [0, 1], so no spread.
    pub(crate) fill_kind: FillKind,
    /// Atlas row when `fill_kind` is a gradient, else ignored.
    pub(crate) fill_lut_row: LutRow,
    /// How the vertex shader reads the geometry lanes.
    pub(crate) kind: CurveKind,
}
