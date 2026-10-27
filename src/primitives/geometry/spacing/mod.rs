//! Per-side padding and margin: four f16 lanes in eight bytes, like `Corners` and `RgbaF16`.

use crate::primitives::geometry::size::Size;
use crate::primitives::math::num::Num;
use crate::primitives::packed::half_simd::F16x4;
use crate::primitives::packed::serde::LaneCodec;
use std::ops;

/// Per-side spacing (padding / margin) as four f16 lanes in `[u16; 4]`, order `left | top | right | bottom`.
///
/// Lossless for integers up to 2048; above that an f16 step is 2 px below 4096 and 4 px below 8192, rounding by up to ±1 px and ±2 px.
///
/// Hash is one `u64` write of the packed form; `LayoutCore::hash_with_flags` folds this twice per node every frame.
#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq, Hash, Default, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Spacing(F16x4);

f16x4_lanes!(Spacing, [left, top, right, bottom]);

impl Spacing {
    /// Packed 8-byte form, used by `LayoutCore::hash_with_flags`.
    #[inline]
    pub(crate) const fn as_u64(self) -> u64 {
        self.0.as_u64()
    }

    /// The same value on all four edges.
    ///
    /// # Panics
    ///
    /// Panics unless `v` packs to a finite f16, as [`Self::new`].
    #[inline]
    #[track_caller]
    pub fn all(v: f32) -> Self {
        Self::from_array([v, v, v, v])
    }

    /// `x` on left and right, `y` on top and bottom (CSS two-value shorthand).
    ///
    /// # Panics
    ///
    /// Panics unless both pack to finite f16s, as [`Self::new`].
    #[inline]
    #[track_caller]
    pub fn xy(x: f32, y: f32) -> Self {
        Self::from_array([x, y, x, y])
    }

    /// Each edge independently, in logical pixels.
    ///
    /// # Panics
    ///
    /// Panics unless every edge packs to a finite f16: finite, and below 65520 in magnitude, which
    /// rounds to 65504. A larger edge would pack to infinity and fail in a later setter, under
    /// another rule.
    #[inline]
    #[track_caller]
    pub fn new(left: f32, top: f32, right: f32, bottom: f32) -> Self {
        Self::from_array([left, top, right, bottom])
    }

    /// Inverse of [`Self::as_array`]: the f32→f16 pack, for hot sites computing all four.
    ///
    /// # Panics
    ///
    /// Panics unless every lane packs to a finite f16, as [`Self::new`].
    #[inline]
    #[track_caller]
    pub fn from_array(v: [f32; 4]) -> Self {
        let packed = F16x4::from_lanes(v);
        assert!(
            !packed.any_lane_non_finite(),
            "{}",
            <Self as LaneCodec>::LANE_RULE
        );
        Self(packed)
    }

    /// A spacing the crate derives from valid lengths (a padding grown by a border, ink past a
    /// glyph run), whose lanes saturate at ±65504 rather than pack to infinity: the lengths have
    /// no upper bound, and a spacing that wide reaches past every window a texture can fill, so
    /// the clamp changes nothing drawn. Authored spacings go through [`Self::new`], which refuses.
    ///
    /// # Panics
    ///
    /// Panics on a NaN lane, which no valid length makes.
    #[inline]
    #[track_caller]
    pub(crate) fn saturating(lanes: [f32; 4]) -> Self {
        Self::from_array(lanes.map(|lane| lane.clamp(-F16x4::MAX_LANE, F16x4::MAX_LANE)))
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

/// Wire format: see [`LaneCodec`]. The 2-node shorthand is `[horizontal, vertical]`.
impl LaneCodec for Spacing {
    const FIELDS: &'static [&'static str] = &["left", "top", "right", "bottom"];

    fn from_lane_array(lanes: [f32; 4]) -> Self {
        Self::from_array(lanes)
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
