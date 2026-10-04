//! Per-corner radii, four f16 lanes in eight bytes — the same packing
//! `Spacing` and `RgbaF16` use, with corner names on the lanes.

use crate::primitives::geometry::size::Size;
use crate::primitives::math::num::Num;
use crate::primitives::packed::half_simd::F16x4;
use crate::primitives::packed::serde::LaneCodec;

/// Per-corner radii, packed as four f16 lanes in a `u64` (8 bytes).
///
/// Lane layout (LE): `tl | tr | br | bl`. As `vec2<u32>` on the GPU
/// the first u32 carries `tl,tr` and the second `br,bl`; the shader
/// reconstructs `vec4<f32>` via two `unpack2x16float` calls.
///
/// Precision: lossless for integer radii up to 2048. Above that an f16
/// step is 2 px below 4096 and 4 px below 8192, so a radius rounds by up
/// to ±1 px and ±2 px there; +Inf above 65504.
///
/// Hash delegates to the packed `F16x4` representation — one `u64` write,
/// fed every frame into
/// `LayoutCore::hash_with_flags` → `SubtreeRollups`.
#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq, Hash, Default, bytemuck::Pod, bytemuck::Zeroable)]
#[must_use]
pub struct Corners(F16x4);

f16x4_lanes!(Corners, [tl, tr, br, bl]);

impl Corners {
    /// Panics unless every radius is a [length](crate::widget::domain::length) the f16
    /// lanes hold, at most 65504 — the check a radius faces where it
    /// enters a shape or a node, under the rule a theme file's radius is
    /// read by. A larger one packed to infinity, so the length rule's
    /// "finite" would blame a value the caller never passed.
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

    /// Every radius multiplied by `scale` — what carries a logical
    /// radius into physical pixels at compose time.
    #[inline]
    pub fn scaled_by(self, scale: f32) -> Self {
        Self(self.0.scaled(scale))
    }

    /// The radii a box of `size` physical px is drawn with: every radius
    /// times `scale`, grown by a shadow's `spread` (zero for anything
    /// else), then reduced so no two adjacent corners overlap. Computed in
    /// f32 and packed once, so a radius too large for f16 at this scale —
    /// `corners(9999)` at 8× — fits the box instead of reaching the shader
    /// as `inf`. After it no radius exceeds half the shorter side, which
    /// the rounded-box SDF assumes; the f16 lanes overflow only for a box
    /// itself wider than 65504 px.
    ///
    /// The spread rule is CSS Backgrounds 3 §7.1 (`box-shadow`): with
    /// `s > 0` a radius `r` becomes `r + s` when `r ≥ s`, and
    /// `r + s·(1 + (r/s − 1)³)` below, so a sharp corner stays sharp;
    /// with `s < 0` it becomes `max(r + s, 0)`. The fit is §5.5
    /// "Overlapping curves": `f` is the least of each side's length over
    /// the sum of its two radii, and every radius scales by `f` when
    /// `f < 1`. `quad_pipeline/shader.wgsl`'s `fit_radii` and `spread_radius` are the
    /// shader's copies, for the inset-shadow hole only the shader sizes.
    pub(crate) fn fit_to(self, size: Size, scale: f32, spread: f32) -> Self {
        let radii = self.as_array().map(|r| spread_radius(r * scale, spread));
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
        Self::from_array(radii.map(|r| r * f))
    }

    /// True when every corner is within UI epsilon of zero. Routes
    /// through `F16x4::all_lanes_noop` (crate-private, in
    /// `primitives::packed::half_simd`) so the lane compare lives in one place —
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

    /// Four raw lane words — see [`F16x4::from_bits`] for the one
    /// non-f16 use.
    #[inline]
    pub(crate) const fn from_bits(bits: [u16; 4]) -> Self {
        Self(F16x4::from_bits(bits))
    }

    /// Packed 8-byte form, the peer of `Spacing::as_u64`. The chrome
    /// hash folds the four radii into one hasher write with it.
    #[inline]
    pub(crate) const fn as_u64(self) -> u64 {
        self.0.as_u64()
    }
}

/// One radius grown by a shadow's `spread` — see [`Corners::fit_to`].
const fn spread_radius(r: f32, spread: f32) -> f32 {
    if spread < 0.0 {
        (r + spread).max(0.0)
    } else if r >= spread {
        r + spread
    } else {
        let t = r / spread - 1.0;
        r + spread * (1.0 + t * t * t)
    }
}

/// `(top, bottom)` — both corners on each edge, the same pairing
/// [`From<Vec2>`] and the wire format's 2-node shorthand use.
impl<T: Num, B: Num> From<(T, B)> for Corners {
    fn from((t, b): (T, B)) -> Self {
        let (t, b) = (t.as_f32(), b.as_f32());
        Self::new(t, t, b, b)
    }
}

/// `(tl, tr, br, bl)` — matches lane order.
impl<TL: Num, TR: Num, BR: Num, BL: Num> From<(TL, TR, BR, BL)> for Corners {
    fn from((tl, tr, br, bl): (TL, TR, BR, BL)) -> Self {
        Self::new(tl.as_f32(), tr.as_f32(), br.as_f32(), bl.as_f32())
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

    const LANE_RULE: &'static str =
        "a corner radius must be finite, not negative, and at most 65504";

    fn lane_is_valid(lane: f32) -> bool {
        (0.0..=F16x4::MAX_LANE).contains(&lane)
    }
}

#[cfg(test)]
mod tests;
