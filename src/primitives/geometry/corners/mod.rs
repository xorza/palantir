//! Per-corner radii, four f16 lanes in eight bytes like `Spacing` and `RgbaF16`.

use crate::primitives::geometry::size::Size;
use crate::primitives::geometry::spacing::Spacing;
use crate::primitives::math::domain;
use crate::primitives::math::num::Number;
use crate::primitives::packed::half_simd::F16x4;
use crate::primitives::packed::serde::{LaneCodec, TwoNode};

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

// Wire format: see `LaneCodec`. No 2-node array: CSS reads one as the two diagonals and
// `Spacing` as horizontal and vertical, so a file names all four corners, or the ones it rounds in
// a table, as WPF's `CornerRadius` takes one value or four.
f16x4_lanes!(
    Corners,
    [tl, tr, br, bl],
    floor = Some(0.0),
    two_node = TwoNode::Refused,
    rule = "a corner radius must be finite, not negative, and round to at most 65504 in f16",
    valid = is_radius_lane,
);

/// A radius a lane holds: a [length](crate::widget::domain::length) and a
/// [lane](crate::widget::domain::lane).
const fn is_radius_lane(lane: f32) -> bool {
    domain::is_length(lane) && domain::is_lane(lane)
}

impl Corners {
    /// The top-left radius.
    pub fn top_left(self) -> f32 {
        self.to_array()[0]
    }

    /// The top-right radius.
    pub fn top_right(self) -> f32 {
        self.to_array()[1]
    }

    /// The bottom-right radius.
    pub fn bottom_right(self) -> f32 {
        self.to_array()[2]
    }

    /// The bottom-left radius.
    pub fn bottom_left(self) -> f32 {
        self.to_array()[3]
    }

    /// Panics unless every radius is a [length](crate::widget::domain::length) and a
    /// [lane](crate::widget::domain::lane). Checked on the packed lanes, where a radius that is no
    /// lane is infinite: the same check as one of the f32 radii.
    #[inline]
    #[track_caller]
    pub(crate) const fn validate(self) {
        assert!(self.is_valid(), "{}", <Self as LaneCodec>::LANE_RULE);
    }

    /// Whether every radius is a length and a lane: [`Self::validate`]'s test.
    #[inline]
    pub(crate) const fn is_valid(self) -> bool {
        !self.0.any_lane_non_finite() & !self.0.any_lane_negative()
    }

    /// Radii the crate derives from valid lengths (half a stroke's width), whose lanes
    /// [saturate](F16x4::saturate_lane) at 65504 rather than pack to infinity: the lengths have no
    /// upper bound, and a radius that large rounds past every box a texture can fill, so the clamp
    /// changes nothing drawn. Authored radii go through [`Self::new`], which refuses.
    ///
    /// # Panics
    ///
    /// Panics on a lane that is no length, as NaN or a negative radius.
    #[inline]
    #[track_caller]
    pub(crate) fn saturating(lanes: [f32; 4]) -> Self {
        Self::checked(lanes.map(F16x4::saturate_lane))
    }

    /// One radius on all four corners.
    ///
    /// # Panics
    ///
    /// As [`Self::new`].
    #[inline]
    #[track_caller]
    pub fn all(r: f32) -> Self {
        Self::checked([r, r, r, r])
    }

    /// Four radii, clockwise from the top left.
    ///
    /// # Panics
    ///
    /// Panics unless every radius is a [length](crate::widget::domain::length), finite and not
    /// negative, and a [lane](crate::widget::domain::lane), below 65520, which rounds to 65504. A
    /// larger radius would pack to infinity.
    #[inline]
    #[track_caller]
    pub fn new(top_left: f32, top_right: f32, bottom_right: f32, bottom_left: f32) -> Self {
        Self::checked([top_left, top_right, bottom_right, bottom_left])
    }

    /// Rounds the top edge only.
    ///
    /// # Panics
    ///
    /// As [`Self::new`].
    #[inline]
    #[track_caller]
    pub fn top(r: f32) -> Self {
        Self::checked([r, r, 0.0, 0.0])
    }

    /// Rounds the bottom edge only.
    ///
    /// # Panics
    ///
    /// As [`Self::new`].
    #[inline]
    #[track_caller]
    pub fn bottom(r: f32) -> Self {
        Self::checked([0.0, 0.0, r, r])
    }

    /// Rounds the left edge only.
    ///
    /// # Panics
    ///
    /// As [`Self::new`].
    #[inline]
    #[track_caller]
    pub fn left(r: f32) -> Self {
        Self::checked([r, 0.0, 0.0, r])
    }

    /// Rounds the right edge only.
    ///
    /// # Panics
    ///
    /// As [`Self::new`].
    #[inline]
    #[track_caller]
    pub fn right(r: f32) -> Self {
        Self::checked([0.0, r, r, 0.0])
    }

    /// Every radius multiplied by `factor`, each product saturating at 65504 rather than packing to
    /// infinity.
    ///
    /// # Panics
    ///
    /// Panics unless `factor` is a [length](crate::widget::domain::length): a negative or NaN
    /// factor makes no radius, and an infinite one makes NaN of a zero radius.
    #[inline]
    #[track_caller]
    pub fn scaled_by(self, factor: f32) -> Self {
        assert!(
            domain::is_length(factor),
            "a corner scale must be finite and not negative, not {factor}"
        );
        Self(self.0.scaled_saturating(factor))
    }

    /// The radii of the box inset from one of `size` by `padding`, as a border insets the
    /// padding edge (CSS Backgrounds 3 §5.2): fitted to the outer box first (§5.5), then each
    /// loses the larger of its two adjacent insets, floored at zero, so the inner curve stays
    /// inside both edges.
    #[inline]
    pub(crate) fn inner(self, size: Size, padding: Spacing) -> Self {
        let [tl, tr, br, bl] = self.fitted_radii(size, 1.0);
        let [left, top, right, bottom] = padding.to_array();
        Self(F16x4::from_lanes([
            (tl - top.max(left)).max(0.0),
            (tr - top.max(right)).max(0.0),
            (br - bottom.max(right)).max(0.0),
            (bl - bottom.max(left)).max(0.0),
        ]))
    }

    /// The radii a box of `size` physical px is drawn with: scaled, then
    /// reduced so adjacent corners don't overlap (CSS Backgrounds 3 §5.5), and
    /// [saturated](F16x4::from_lanes_saturating) where a box past 131008 px still leaves one past
    /// the lanes. `fit_radii` in `quad_pipeline/shadow.wgsl` is the shader's copy.
    pub(crate) fn fit_to(self, size: Size, scale: f32) -> Self {
        Self(F16x4::from_lanes_saturating(self.fitted_radii(size, scale)))
    }

    /// [`Self::fit_to`] before packing.
    fn fitted_radii(self, size: Size, scale: f32) -> [f32; 4] {
        fit(self.to_array().map(|r| r * scale), size)
    }

    /// True when every corner is within UI epsilon of zero; `&self` for serde's `skip_serializing_if`.
    #[inline]
    pub const fn is_approx_zero(&self) -> bool {
        self.0.all_lanes_noop()
    }

    #[inline]
    pub(crate) const fn as_u64(self) -> u64 {
        self.0.as_u64()
    }
}

/// `(tl, tr, br, bl)`, in lane order, as [`Corners::new`] and CSS `border-radius` take them.
impl<TL: Number, TR: Number, BR: Number, BL: Number> From<(TL, TR, BR, BL)> for Corners {
    #[track_caller]
    fn from((tl, tr, br, bl): (TL, TR, BR, BL)) -> Self {
        Self::new(tl.to_f32(), tr.to_f32(), br.to_f32(), bl.to_f32())
    }
}

/// `radii` (tl, tr, br, bl) on a box of `size`, all scaled by one factor so no two adjacent ones
/// overlap (CSS Backgrounds 3 §5.5): the CPU's one copy, beside the shader's `fit_radii`.
pub(crate) fn fit(radii: [f32; 4], size: Size) -> [f32; 4] {
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

#[cfg(test)]
mod tests;
