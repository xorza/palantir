//! Per-side padding and margin, four f16 lanes in eight bytes — the same
//! packing `Corners` and `RgbaF16` use, with edge names on the lanes.

use crate::primitives::geometry::size::Size;
use crate::primitives::math::num::Num;
use crate::primitives::packed::half_simd::F16x4;
use crate::primitives::packed::serde::LaneCodec;
use std::ops;

/// Per-side spacing (padding / margin), packed as four f16 lanes in
/// `[u16; 4]` (8 bytes). Lane order: `left | top | right | bottom`.
///
/// Precision: lossless for integer values up to 2048. Above that an f16
/// step is 2 px below 4096 and 4 px below 8192, so a value rounds by up to
/// ±1 px and ±2 px there. UI spacing never approaches the f16 ceiling.
///
/// Hash delegates to the packed `F16x4` representation (one `u64` write) —
/// `LayoutCore::hash_with_flags` folds this twice per node every frame (padding + margin),
/// so the single-write form matters.
#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq, Hash, Default, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Spacing(F16x4);

f16x4_lanes!(Spacing, [left, top, right, bottom]);

impl Spacing {
    /// Packed 8-byte form. Used by `LayoutCore::hash_with_flags` to fold the
    /// padding + margin lanes into the parent hasher write.
    #[inline]
    pub(crate) const fn as_u64(self) -> u64 {
        self.0.as_u64()
    }
}

impl Spacing {
    /// The same value on all four edges.
    #[inline]
    pub fn all(v: f32) -> Self {
        Self(F16x4::from_lanes([v, v, v, v]))
    }

    /// `x` on left and right, `y` on top and bottom — the CSS two-value
    /// shorthand.
    #[inline]
    pub fn xy(x: f32, y: f32) -> Self {
        Self(F16x4::from_lanes([x, y, x, y]))
    }

    /// Each edge independently, in logical pixels.
    #[inline]
    pub fn new(left: f32, top: f32, right: f32, bottom: f32) -> Self {
        Self(F16x4::from_lanes([left, top, right, bottom]))
    }

    /// Inverse of [`Self::as_array`] — the four-lane f32→f16 pack. Use at
    /// hot sites that compute all four.
    #[inline]
    pub fn from_array(v: [f32; 4]) -> Self {
        Self(F16x4::from_lanes(v))
    }

    /// The size this spacing costs: `w = left + right`, `h = top + bottom`.
    #[inline]
    pub fn sums(self) -> Size {
        let [l, t, r, b] = self.as_array();
        Size::new(l + r, t + b)
    }
}

impl ops::Add for Spacing {
    type Output = Self;
    #[inline]
    fn add(self, rhs: Self) -> Self {
        let [al, at, ar, ab] = self.as_array();
        let [bl, bt, br, bb] = rhs.as_array();
        Self::from_array([al + bl, at + bt, ar + br, ab + bb])
    }
}

/// `(horizontal, vertical)` — both sides on each axis.
impl<X: Num, Y: Num> From<(X, Y)> for Spacing {
    fn from((x, y): (X, Y)) -> Self {
        Self::xy(x.as_f32(), y.as_f32())
    }
}

/// `(left, top, right, bottom)` — matches struct field order.
impl<L: Num, T: Num, R: Num, B: Num> From<(L, T, R, B)> for Spacing {
    fn from((l, t, r, b): (L, T, R, B)) -> Self {
        Self::new(l.as_f32(), t.as_f32(), r.as_f32(), b.as_f32())
    }
}

/// Wire format: see [`LaneCodec`] — a scalar, a 1/2/4-node array, or a
/// `{left, top, right, bottom}` table. The 2-node shorthand is
/// `[horizontal, vertical]`, matching CSS's two-value padding.
impl LaneCodec for Spacing {
    const FIELDS: &'static [&'static str] = &["left", "top", "right", "bottom"];

    fn from_lane_array(lanes: [f32; 4]) -> Self {
        Self::new(lanes[0], lanes[1], lanes[2], lanes[3])
    }

    fn to_lane_array(&self) -> [f32; 4] {
        self.as_array()
    }

    fn two_form(lanes: [f32; 4]) -> Option<[f32; 2]> {
        (lanes[0] == lanes[2] && lanes[1] == lanes[3]).then_some([lanes[0], lanes[1]])
    }

    fn expand_two([horizontal, vertical]: [f32; 2]) -> [f32; 4] {
        [horizontal, vertical, horizontal, vertical]
    }

    const LANE_RULE: &'static str = "a spacing lane must be finite and within ±65504";

    fn lane_is_valid(lane: f32) -> bool {
        lane.abs() <= F16x4::MAX_LANE
    }
}

#[cfg(test)]
mod tests;
