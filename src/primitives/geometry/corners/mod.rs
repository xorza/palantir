//! Per-corner radii, four f16 lanes in eight bytes like `Spacing` and `RgbaF16`.

use crate::primitives::geometry::size::Size;
use crate::primitives::math::num::Num;
use crate::primitives::packed::half_simd::F16x4;
use crate::primitives::packed::serde::LaneCodec;

/// Per-corner radii, packed as four f16 lanes in a `u64`.
///
/// Lane layout (LE): `tl | tr | br | bl`. On the GPU it is a `vec2<u32>`:
/// `tl,tr` then `br,bl`.
///
/// Precision: lossless for integer radii up to 2048; an f16 step is 2 px
/// below 4096 and 4 px below 8192; +Inf above 65504.
#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq, Hash, Default, bytemuck::Pod, bytemuck::Zeroable)]
#[must_use]
pub struct Corners(F16x4);

f16x4_lanes!(Corners, [tl, tr, br, bl]);

impl Corners {
    /// Panics unless every radius is a [length](crate::widget::domain::length)
    /// the f16 lanes hold (at most 65504); a larger one would pack to infinity.
    #[inline]
    #[track_caller]
    pub(crate) const fn validate(self) {
        assert!(
            !self.0.any_lane_non_finite() && !self.0.any_lane_negative(),
            "{}",
            <Self as LaneCodec>::LANE_RULE,
        );
    }

    /// One radius on all four corners.
    #[inline]
    pub fn all(r: f32) -> Self {
        Self(F16x4::from_lanes([r, r, r, r]))
    }

    /// Four radii, clockwise from the top left.
    #[inline]
    pub fn new(top_left: f32, top_right: f32, bottom_right: f32, bottom_left: f32) -> Self {
        Self(F16x4::from_lanes([
            top_left,
            top_right,
            bottom_right,
            bottom_left,
        ]))
    }

    /// Rounds the top edge only.
    #[inline]
    pub fn top(r: f32) -> Self {
        Self(F16x4::from_lanes([r, r, 0.0, 0.0]))
    }

    /// Rounds the bottom edge only.
    #[inline]
    pub fn bottom(r: f32) -> Self {
        Self(F16x4::from_lanes([0.0, 0.0, r, r]))
    }

    /// Rounds the left edge only.
    #[inline]
    pub fn left(r: f32) -> Self {
        Self(F16x4::from_lanes([r, 0.0, 0.0, r]))
    }

    /// Rounds the right edge only.
    #[inline]
    pub fn right(r: f32) -> Self {
        Self(F16x4::from_lanes([0.0, r, r, 0.0]))
    }

    /// Every radius multiplied by `factor`.
    #[inline]
    pub fn scaled_by(self, factor: f32) -> Self {
        Self(self.0.scaled(factor))
    }

    /// The radii of the padding edge under a border `width` (CSS Backgrounds 3
    /// §5.5/§5.2): fitted to the box, then each loses `width`, floored at zero.
    #[inline]
    pub(crate) fn deflated(self, size: Size, width: f32) -> Self {
        let fitted = self.fitted_radii(size, 1.0);
        Self(F16x4::from_lanes(fitted.map(|r| (r - width).max(0.0))))
    }

    /// The radii a box of `size` physical px is drawn with: scaled, then
    /// reduced so adjacent corners don't overlap (CSS Backgrounds 3 §5.5).
    /// Packed once from f32, so a radius too large for f16 fits the box instead
    /// of reaching the shader as `inf`. `fit_radii` in `shader.wgsl` is the shader's copy.
    pub(crate) fn fit_to(self, size: Size, scale: f32) -> Self {
        Self(F16x4::from_lanes(self.fitted_radii(size, scale)))
    }

    /// [`Self::fit_to`] before packing.
    fn fitted_radii(self, size: Size, scale: f32) -> [f32; 4] {
        let radii = self.as_array().map(|r| r * scale);
        let [tl, tr, br, bl] = radii;
        let f = [
            (size.w, tl + tr),
            (size.w, bl + br),
            (size.h, tl + bl),
            (size.h, tr + br),
        ]
        .into_iter()
        .filter(|&(_, sum)| sum > 0.0)
        .fold(1.0_f32, |f, (side, sum)| f.min(side.max(0.0) / sum));
        radii.map(|r| r * f)
    }

    /// True when every corner is within UI epsilon of zero; `&self` for serde's `skip_serializing_if`.
    #[inline]
    pub const fn is_approx_zero(&self) -> bool {
        self.0.all_lanes_noop()
    }

    /// Four raw lane words; see [`F16x4::from_bits`].
    #[inline]
    pub(crate) const fn from_bits(bits: [u16; 4]) -> Self {
        Self(F16x4::from_bits(bits))
    }

    #[inline]
    pub(crate) const fn as_u64(self) -> u64 {
        self.0.as_u64()
    }
}

/// `(top, bottom)`: both corners on each edge.
impl<T: Num, B: Num> From<(T, B)> for Corners {
    fn from((t, b): (T, B)) -> Self {
        let (t, b) = (t.as_f32(), b.as_f32());
        Self::new(t, t, b, b)
    }
}

/// `(tl, tr, br, bl)`, in lane order.
impl<TL: Num, TR: Num, BR: Num, BL: Num> From<(TL, TR, BR, BL)> for Corners {
    fn from((tl, tr, br, bl): (TL, TR, BR, BL)) -> Self {
        Self::new(tl.as_f32(), tr.as_f32(), br.as_f32(), bl.as_f32())
    }
}

/// Wire format: see [`LaneCodec`]. The 2-node shorthand is `[top, bottom]`.
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

    const LANE_RULE: &'static str =
        "a corner radius must be finite, not negative, and at most 65504";

    fn lane_is_valid(lane: f32) -> bool {
        (0.0..=F16x4::MAX_LANE).contains(&lane)
    }
}

#[cfg(test)]
mod tests;
