//! How one axis of a node resolves: a fixed extent, a share of what is
//! left, or whatever its content needs.

use crate::primitives::geometry::size::Size;
use crate::primitives::math::domain;
use crate::primitives::math::float_hash::{self, FloatHash};
use crate::primitives::math::num::Num;
use glam::BVec2;
use std::fmt;
use std::hash;

/// How one axis of a node resolves during layout.
///
/// WPF-style: Fixed = exact px, Hug = Auto, Fill = Star (remainder shared by
/// `weight` across Fill siblings).
///
/// The floor under Hug and Fill is the node's intrinsic minimum (a fixed
/// descendant, an explicit `min_size`, the longest unbreakable word); a Hug
/// axis is also floored at what its content takes at the size it is laid out
/// at. The ceiling is `max_size`.
///
/// Siblings sharing an axis split it the same way in every container: each
/// Fill's floor is set aside first; Hug participants share what is left,
/// giving way toward their floors in proportion to how far each can give; Fill
/// participants divide the rest by `weight`, taking any bound their share
/// violates while the rest re-divide. A parent never grows to fit a child.
///
/// # Which constructors panic, and why
///
/// [`Self::fixed`] and [`Self::fill`] panic on a negative or non-finite value:
/// clamping would hide the upstream arithmetic that produced it.
/// [`Self::split`] is total, since widgets feed it numbers from application
/// code.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Sizing(SizingValue);

#[derive(Clone, Copy, Debug, PartialEq, Default)]
enum SizingValue {
    Fixed(f32),
    #[default]
    Hug,
    Fill(f32),
}

impl Sizing {
    /// Shrink-wrap the content: `min(content, available)`, floored at the
    /// smallest extent the content takes at the size it is laid out at.
    /// Content that can give way, such as a scroll on its panned axis, still
    /// shrinks. The default.
    pub const HUG: Self = Self(SizingValue::Hug);
    /// Take the leftover space at weight `1.0`.
    pub const FILL: Self = Self::fill(1.0);

    /// An exact pixel extent. `value`: a *length*.
    ///
    /// # Panics
    ///
    /// Panics unless `value` is a [length](crate::widget::domain::length).
    #[inline]
    #[track_caller]
    pub const fn fixed(value: f32) -> Self {
        Self(SizingValue::Fixed(domain::length(value)))
    }

    /// A share of remaining space in proportion to `weight` across `Fill`
    /// siblings. `weight`: a *length*.
    ///
    /// A zero weight takes no share (as WPF's `0*`) and is stored as
    /// `fixed(0.0)`, so [`Self::fill_weight`] answers `None` for it.
    ///
    /// # Panics
    ///
    /// Panics unless `weight` is a [length](crate::widget::domain::length).
    #[inline]
    #[track_caller]
    pub const fn fill(weight: f32) -> Self {
        let weight = domain::length(weight);
        if weight == 0.0 {
            Self(SizingValue::Fixed(0.0))
        } else {
            Self(SizingValue::Fill(weight))
        }
    }

    /// Split the parent's space into `[fraction, 1 - fraction]` shares, as
    /// `Fill` weights resolved at arrange (used by `ProgressBar` and `Slider`).
    ///
    /// Total over every `f32`: `fraction` goes through `domain::fraction`, so
    /// an endpoint collapses a share to a zero-extent `Fixed` and a NaN reads
    /// as empty.
    pub const fn split(fraction: f32) -> [Self; 2] {
        let f = domain::fraction(fraction);
        [Self::fill(f), Self::fill(1.0 - f)]
    }

    /// The pixel extent if this is a [`Self::fixed`], else `None`.
    #[inline]
    pub const fn fixed_value(self) -> Option<f32> {
        match self.0 {
            SizingValue::Fixed(value) => Some(value),
            SizingValue::Hug | SizingValue::Fill(_) => None,
        }
    }

    /// The weight if this is a [`Self::fill`], else `None`.
    #[inline]
    pub const fn fill_weight(self) -> Option<f32> {
        match self.0 {
            SizingValue::Fill(weight) => Some(weight),
            SizingValue::Fixed(_) | SizingValue::Hug => None,
        }
    }

    /// `true` for [`Self::HUG`].
    #[inline]
    pub const fn is_hug(self) -> bool {
        matches!(self.0, SizingValue::Hug)
    }

    /// Feed the whole value through `bits` in one `u64` write: tag in the low
    /// byte, canonicalized payload above it. A raw `bytes_of` would hash the
    /// padding of the inactive variant.
    #[inline]
    pub(crate) fn hash_bits<H: hash::Hasher, F: Fn(f32) -> u32>(self, h: &mut H, bits: F) {
        let (tag, value) = match self.0 {
            SizingValue::Fixed(value) => (0u8, value),
            SizingValue::Hug => (1, 0.0),
            SizingValue::Fill(value) => (2, value),
        };
        h.write_u64(u64::from(tag) | (u64::from(bits(value)) << 8));
    }
}

impl FloatHash for Sizing {
    #[inline]
    fn hash_eq<H: hash::Hasher>(&self, h: &mut H) {
        self.hash_bits(h, float_hash::eq_bits);
    }

    #[inline]
    fn hash_visual<H: hash::Hasher>(&self, h: &mut H) {
        self.hash_bits(h, float_hash::canon_bits);
    }
}

impl<T: Num> From<T> for Sizing {
    fn from(v: T) -> Self {
        Sizing::fixed(v.as_f32())
    }
}

impl hash::Hash for Sizing {
    #[inline]
    fn hash<H: hash::Hasher>(&self, h: &mut H) {
        self.hash_eq(h);
    }
}

/// Per-axis `Sizing`, packed into 8 B (two `u32` slots). Each slot holds the
/// value's `f32` bits; the tag rides in bit patterns no other variant holds:
///
/// - `Fixed(v)` is `v` itself: non-negative, with `-0.0` folded to `+0.0`.
/// - `Fill(w)` is `-w`: the weight is positive, so the sign bit is set.
/// - `Hug` is `+∞`, which both others exclude.
///
/// `Configure::size` takes `impl Into<SizeSpec>`: `.size(100.0)`,
/// `.size(Sizing::FILL)`, `.size((Sizing::FILL, 40.0))`.
#[derive(Clone, Copy)]
pub struct SizeSpec {
    w_packed: u32,
    h_packed: u32,
}

impl Default for SizeSpec {
    #[inline]
    fn default() -> Self {
        Self::new(Sizing::HUG, Sizing::HUG)
    }
}

/// `Hug`'s slot; see [`SizeSpec`].
const HUG_BITS: u32 = f32::INFINITY.to_bits();

#[inline]
const fn encode_sizing(s: Sizing) -> u32 {
    match s.0 {
        SizingValue::Fixed(value) => float_hash::eq_bits(value),
        SizingValue::Hug => HUG_BITS,
        SizingValue::Fill(weight) => (-weight).to_bits(),
    }
}

#[inline]
const fn decode_sizing(packed: u32) -> Sizing {
    let value = f32::from_bits(packed);
    if packed == HUG_BITS {
        Sizing::HUG
    } else if value.is_sign_negative() {
        Sizing(SizingValue::Fill(-value))
    } else {
        Sizing(SizingValue::Fixed(value))
    }
}

impl SizeSpec {
    /// Both axes, packed into eight bytes.
    #[inline]
    pub const fn new(w: Sizing, h: Sizing) -> Self {
        Self {
            w_packed: encode_sizing(w),
            h_packed: encode_sizing(h),
        }
    }
    /// Packed 8-byte form: `w_packed` low, `h_packed` high.
    #[inline]
    pub(crate) const fn as_u64(self) -> u64 {
        ((self.h_packed as u64) << 32) | self.w_packed as u64
    }
    /// The horizontal axis.
    #[inline]
    pub const fn w(self) -> Sizing {
        decode_sizing(self.w_packed)
    }
    /// The vertical axis.
    #[inline]
    pub const fn h(self) -> Sizing {
        decode_sizing(self.h_packed)
    }

    /// Which axes hug their content, as a lane mask for
    /// [`Size::select`](crate::primitives::geometry::size::Size::select).
    #[inline]
    pub(crate) const fn hug_mask(self) -> BVec2 {
        BVec2::new(self.w().is_hug(), self.h().is_hug())
    }
}

impl PartialEq for SizeSpec {
    #[inline]
    fn eq(&self, other: &Self) -> bool {
        self.w_packed == other.w_packed && self.h_packed == other.h_packed
    }
}

impl fmt::Debug for SizeSpec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SizeSpec")
            .field("w", &self.w())
            .field("h", &self.h())
            .finish()
    }
}

impl From<Sizing> for SizeSpec {
    fn from(s: Sizing) -> Self {
        Self::new(s, s)
    }
}

impl hash::Hash for SizeSpec {
    #[inline]
    fn hash<H: hash::Hasher>(&self, h: &mut H) {
        h.write_u64(self.as_u64());
    }
}

impl<T: Num> From<T> for SizeSpec {
    fn from(v: T) -> Self {
        Sizing::from(v).into()
    }
}

impl<W: Into<Sizing>, H: Into<Sizing>> From<(W, H)> for SizeSpec {
    fn from((w, h): (W, H)) -> Self {
        Self::new(w.into(), h.into())
    }
}

impl From<Size> for SizeSpec {
    fn from(s: Size) -> Self {
        (s.w, s.h).into()
    }
}

#[cfg(test)]
mod tests {
    use crate::internals::panic_probe;
    use crate::primitives::layout::sizing::{SizeSpec, Sizing};
    use crate::primitives::math::domain;

    /// The two shares always partition 1.0, an out-of-range input clamps, and
    /// an endpoint is a zero-extent `Fixed`, not a zero-weight `Fill`.
    #[test]
    fn split_partitions_one_and_clamps_out_of_range() {
        let cases = [
            (0.0, 0.0, 1.0),
            (0.25, 0.25, 0.75),
            (0.5, 0.5, 0.5),
            (1.0, 1.0, 0.0),
            (-0.3, 0.0, 1.0), // below range clamps to empty
            (1.7, 1.0, 0.0),  // above range clamps to full
            // No share at all reads as empty.
            (f32::NAN, 0.0, 1.0),
            (f32::INFINITY, 0.0, 1.0),
            (f32::NEG_INFINITY, 0.0, 1.0),
        ];
        for (input, want_a, want_b) in cases {
            let got = Sizing::split(input);
            let want = [Sizing::fill(want_a), Sizing::fill(want_b)];
            assert_eq!(got, want, "fraction {input}");
        }
        assert_eq!(
            Sizing::split(0.0)[0],
            Sizing::fixed(0.0),
            "a zero share is a zero-extent Fixed, not a zero-weight Fill",
        );
    }

    /// Every `Fixed` extent and `Fill` weight round-trips bit for bit, beside a
    /// `Hug` on the other axis. Values span zero, the smallest subnormal and
    /// normal, fractions a lossy packing would drop (`4097.7`), and the
    /// largest finite value. `-0.0` folds to `+0.0`.
    #[test]
    fn packing_round_trips_every_value_exactly() {
        let values = [
            0.0,
            -0.0,
            f32::from_bits(1),
            f32::MIN_POSITIVE,
            0.1,
            1279.9,
            4097.7,
            f32::MAX,
        ];
        for v in values {
            let folded = if v == 0.0 { 0.0f32 } else { v };
            let fixed = SizeSpec::new(Sizing::fixed(v), Sizing::HUG);
            assert_eq!(
                fixed.w().fixed_value().map(f32::to_bits),
                Some(folded.to_bits()),
                "Fixed({v:e})",
            );
            assert!(fixed.h().is_hug(), "the Hug beside Fixed({v:e})");
            if v > 0.0 {
                let fill = SizeSpec::new(Sizing::HUG, Sizing::fill(v));
                assert_eq!(
                    fill.h().fill_weight().map(f32::to_bits),
                    Some(v.to_bits()),
                    "Fill({v:e})",
                );
                assert!(fill.w().is_hug(), "the Hug beside Fill({v:e})");
            }
        }
    }

    #[test]
    fn constructors_accept_only_finite_valid_payloads() {
        const FIXED: &str = domain::LENGTH_RULE;
        const FILL: &str = domain::LENGTH_RULE;
        type Case = (&'static str, fn() -> Sizing);

        assert_eq!(Sizing::fixed(f32::MAX).fixed_value(), Some(f32::MAX));
        assert_eq!(Sizing::fill(f32::MAX).fill_weight(), Some(f32::MAX));
        assert_eq!(Sizing::fill(0.0), Sizing::fixed(0.0), "0* takes no share");
        assert_eq!(Sizing::fill(-0.0), Sizing::fixed(0.0));
        assert_eq!(Sizing::fill(0.0).fill_weight(), None);
        assert_eq!(Sizing::fill(2.5).fill_weight(), Some(2.5));

        let cases: &[Case] = &[
            (FIXED, || Sizing::fixed(-1.0)),
            (FIXED, || Sizing::fixed(f32::NAN)),
            (FIXED, || Sizing::fixed(f32::INFINITY)),
            (FIXED, || Sizing::fixed(f32::NEG_INFINITY)),
            (FILL, || Sizing::fill(-1.0)),
            (FILL, || Sizing::fill(f32::NAN)),
            (FILL, || Sizing::fill(f32::INFINITY)),
        ];
        for &(expected, construct) in cases {
            panic_probe::assert_panics_with(expected, construct);
        }
    }
}
