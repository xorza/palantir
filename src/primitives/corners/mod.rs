//! Per-corner radii, four f16 lanes in eight bytes — the same packing
//! `Spacing` and `ColorF16` use, with corner names on the lanes.

use crate::primitives::half_simd::F16x4;
use crate::primitives::serde::LaneCodec;
use crate::primitives::size::Size;
use glam::Vec2;

/// Per-corner radii, packed as four f16 lanes in a `u64` (8 bytes).
///
/// Lane layout (LE): `tl | tr | br | bl`. As `vec2<u32>` on the GPU
/// the first u32 carries `tl,tr` and the second `br,bl`; the shader
/// reconstructs `vec4<f32>` via two `unpack2x16float` calls.
///
/// Precision: lossless for integer radii up to 2048, ~0.25 px error at
/// 4096, +Inf above ~65504. Plenty of headroom for UI workloads.
///
/// Hash delegates to the packed `F16x4` representation — one `u64` write,
/// fed every frame into
/// `LayoutCore::hash` → `SubtreeRollups`.
#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq, Hash, Default, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Corners(F16x4);

f16x4_lanes!(Corners, [tl, tr, br, bl]);

impl Corners {
    #[inline]
    pub fn all(r: f32) -> Self {
        Self(F16x4::from_lanes([r, r, r, r]))
    }

    #[inline]
    pub fn new(tl: f32, tr: f32, br: f32, bl: f32) -> Self {
        Self(F16x4::from_lanes([tl, tr, br, bl]))
    }

    /// Round the top edge only — `tl == tr == r`, `br == bl == 0`.
    #[inline]
    pub fn top(r: f32) -> Self {
        Self(F16x4::from_lanes([r, r, 0.0, 0.0]))
    }

    /// Round the bottom edge only.
    #[inline]
    pub fn bottom(r: f32) -> Self {
        Self(F16x4::from_lanes([0.0, 0.0, r, r]))
    }

    /// Round the left edge only.
    #[inline]
    pub fn left(r: f32) -> Self {
        Self(F16x4::from_lanes([r, 0.0, 0.0, r]))
    }

    /// Round the right edge only.
    #[inline]
    pub fn right(r: f32) -> Self {
        Self(F16x4::from_lanes([0.0, r, r, 0.0]))
    }

    /// CSS-style `[top, bottom]` shorthand.
    #[inline]
    pub fn top_bottom(top: f32, bottom: f32) -> Self {
        Self(F16x4::from_lanes([top, top, bottom, bottom]))
    }

    /// Round the `tl`/`br` diagonal pair (e.g. asymmetric chat bubble).
    #[inline]
    pub fn diag_main(r: f32) -> Self {
        Self(F16x4::from_lanes([r, 0.0, r, 0.0]))
    }

    /// Round the `tr`/`bl` diagonal pair.
    #[inline]
    pub fn diag_anti(r: f32) -> Self {
        Self(F16x4::from_lanes([0.0, r, 0.0, r]))
    }

    #[inline]
    pub fn scaled_by(self, scale: f32) -> Self {
        Self(self.0.scaled(scale))
    }

    /// True when every corner is within UI epsilon of zero. Routes
    /// through `F16x4::all_lanes_noop` (crate-private, in
    /// `primitives::half_simd`) so the lane compare lives in one place —
    /// see that method for the SWAR rationale.
    /// `&self` where its neighbours take `self`: serde's
    /// `skip_serializing_if` requires `fn(&T) -> bool`, and
    /// [`Background::corners`](crate::Background) uses this as one.
    #[inline]
    pub const fn approx_zero(&self) -> bool {
        // A NaN radius reports non-zero and so cannot take the
        // sharp-corner fast path this gates. The shape-level NaN gate is
        // what drops such a shape.
        self.0.all_lanes_noop()
    }
}

impl From<Vec2> for Corners {
    fn from(v: Vec2) -> Self {
        Self::new(v.x, v.x, v.y, v.y)
    }
}

impl From<Size> for Corners {
    fn from(s: Size) -> Self {
        Self::new(s.w, s.w, s.h, s.h)
    }
}

/// Wire format: see [`LaneCodec`] — a scalar, a 1/2/4-node array, or a
/// `{tl, tr, br, bl}` table. The 2-node shorthand is `[top, bottom]`,
/// since a rounded box overwhelmingly varies by edge rather than by
/// diagonal.
impl LaneCodec for Corners {
    const FIELDS: &'static [&'static str] = &["tl", "tr", "br", "bl"];

    fn from_lane_array(lanes: [f32; 4]) -> Self {
        Self::new(lanes[0], lanes[1], lanes[2], lanes[3])
    }

    fn to_lane_array(&self) -> [f32; 4] {
        self.as_array()
    }

    fn two_form(lanes: [f32; 4]) -> Option<[f32; 2]> {
        (lanes[0] == lanes[1] && lanes[2] == lanes[3]).then_some([lanes[0], lanes[2]])
    }

    fn expand_two([top, bottom]: [f32; 2]) -> [f32; 4] {
        [top, top, bottom, bottom]
    }
}

#[cfg(test)]
mod tests;
