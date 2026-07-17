use crate::primitives::{approx, num::Num, size::Size};

/// WPF-style sizing. Maps to: Fixed = exact px, Hug = Auto (use desired),
/// Fill = Star (take remainder, distributed by `weight` across Fill siblings).
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
    pub const HUG: Self = Self(SizingValue::Hug);
    pub const FILL: Self = Self::fill(1.0);

    /// An exact pixel extent.
    ///
    /// # Panics
    ///
    /// Panics if `value` is negative or non-finite.
    #[inline]
    pub const fn fixed(value: f32) -> Self {
        assert!(
            value.is_finite() && value >= 0.0,
            "fixed sizing must be finite and non-negative",
        );
        Self(SizingValue::Fixed(value))
    }

    /// A positive relative share of remaining space.
    ///
    /// # Panics
    ///
    /// Panics if `weight` is zero, negative, or non-finite.
    #[inline]
    pub const fn fill(weight: f32) -> Self {
        assert!(
            weight.is_finite() && weight > 0.0,
            "fill weight must be finite and positive",
        );
        Self(SizingValue::Fill(weight))
    }

    /// A relative share that may be zero. Zero becomes `fixed(0.0)`;
    /// positive values become [`Self::fill`].
    ///
    /// # Panics
    ///
    /// Panics if `weight` is negative or non-finite.
    #[inline]
    pub const fn share(weight: f32) -> Self {
        assert!(
            weight.is_finite() && weight >= 0.0,
            "share weight must be finite and non-negative",
        );
        if weight == 0.0 {
            Self(SizingValue::Fixed(0.0))
        } else {
            Self(SizingValue::Fill(weight))
        }
    }

    #[inline]
    pub const fn fixed_value(self) -> Option<f32> {
        match self.0 {
            SizingValue::Fixed(value) => Some(value),
            SizingValue::Hug | SizingValue::Fill(_) => None,
        }
    }

    #[inline]
    pub const fn fill_weight(self) -> Option<f32> {
        match self.0 {
            SizingValue::Fill(weight) => Some(weight),
            SizingValue::Fixed(_) | SizingValue::Hug => None,
        }
    }

    #[inline]
    pub const fn is_hug(self) -> bool {
        matches!(self.0, SizingValue::Hug)
    }

    #[inline]
    pub(crate) fn hash_visual<H: std::hash::Hasher>(&self, h: &mut H) {
        let (tag, value) = match self.0 {
            SizingValue::Fixed(value) => (0u8, value),
            SizingValue::Hug => (1, 0.0),
            SizingValue::Fill(value) => (2, value),
        };
        h.write_u64((tag as u64) | ((approx::canon_bits(value) as u64) << 8));
    }
}

impl<T: Num> From<T> for Sizing {
    fn from(v: T) -> Self {
        Sizing::fixed(v.as_f32())
    }
}

/// Tagged-union with niche-uninit padding in the inactive variant — raw
/// `bytes_of` would hash junk. Encode `tag:u8 + value:f32` into one
/// `u64` write (tag low, value bits high 32) instead of two small calls.
impl std::hash::Hash for Sizing {
    #[inline]
    fn hash<H: std::hash::Hasher>(&self, h: &mut H) {
        let (tag, v) = match self.0 {
            SizingValue::Fixed(v) => (0u8, v),
            SizingValue::Hug => (1, 0.0),
            SizingValue::Fill(w) => (2, w),
        };
        h.write_u64((tag as u64) | ((approx::eq_bits(v) as u64) << 8));
    }
}

/// Per-axis `Sizing`, packed into 8 B (two `u32` slots). Each slot
/// encodes one `Sizing`: top 2 bits = tag (0=Fixed, 1=Hug, 2=Fill),
/// low 30 bits = the high 30 bits of the payload `f32`. Drops 2
/// mantissa bits — ULP at 1 px ≈ 1e-7, at 1280 px ≈ 1e-3 — well below
/// physical-pixel snapping resolution. Saves 8 B per `LayoutCore`
/// (56 → 48) across the per-node SoA column.
///
/// Construct via `Default` (Hug × Hug), `Sizes::from(s)` (uniform),
/// `Sizes::from(n)` (uniform Fixed via `Num`), or `Sizes::from((w, h))`
/// for asymmetric. The `From` impls are the public surface —
/// `Configure::size` takes `impl Into<Sizes>` so call sites stay terse:
/// `.size(100.0)`, `.size(Sizing::FILL)`, `.size((Sizing::FILL, 40.0))`.
/// Read components via `Sizes::w()` / `Sizes::h()`.
#[derive(Clone, Copy)]
pub struct Sizes {
    w_packed: u32,
    h_packed: u32,
}

impl Default for Sizes {
    #[inline]
    fn default() -> Self {
        Self::new(Sizing::HUG, Sizing::HUG)
    }
}

const SIZING_TAG_FIXED: u32 = 0;
const SIZING_TAG_HUG: u32 = 1;
const SIZING_TAG_FILL: u32 = 2;
const SIZING_TAG_SHIFT: u32 = 30;
const SIZING_VAL_MASK: u32 = (1 << 30) - 1;

#[inline]
const fn encode_sizing(s: Sizing) -> u32 {
    match s.0 {
        SizingValue::Fixed(value) => {
            (SIZING_TAG_FIXED << SIZING_TAG_SHIFT) | (approx::eq_bits(value) >> 2)
        }
        SizingValue::Hug => SIZING_TAG_HUG << SIZING_TAG_SHIFT,
        SizingValue::Fill(weight) => {
            let payload = approx::eq_bits(weight) >> 2;
            // Quantization must not turn a positive Fill into zero.
            let payload = if payload == 0 { 1 } else { payload };
            (SIZING_TAG_FILL << SIZING_TAG_SHIFT) | payload
        }
    }
}

#[inline]
const fn decode_sizing(packed: u32) -> Sizing {
    let tag = packed >> SIZING_TAG_SHIFT;
    let val = f32::from_bits((packed & SIZING_VAL_MASK) << 2);
    match tag {
        SIZING_TAG_FIXED => Sizing(SizingValue::Fixed(val)),
        SIZING_TAG_HUG => Sizing::HUG,
        SIZING_TAG_FILL => Sizing(SizingValue::Fill(val)),
        // Tag 3 is unconstructible by `encode_sizing`.
        _ => unreachable!(),
    }
}

impl Sizes {
    #[inline]
    pub const fn new(w: Sizing, h: Sizing) -> Self {
        Self {
            w_packed: encode_sizing(w),
            h_packed: encode_sizing(h),
        }
    }
    /// Packed 8-byte form: `w_packed` low, `h_packed` high. Used by
    /// `LayoutCore::hash` to fold size into a single hasher write.
    #[inline]
    pub(crate) const fn as_u64(self) -> u64 {
        ((self.h_packed as u64) << 32) | self.w_packed as u64
    }
    #[inline]
    pub const fn w(self) -> Sizing {
        decode_sizing(self.w_packed)
    }
    #[inline]
    pub const fn h(self) -> Sizing {
        decode_sizing(self.h_packed)
    }
}

impl PartialEq for Sizes {
    #[inline]
    fn eq(&self, other: &Self) -> bool {
        self.w_packed == other.w_packed && self.h_packed == other.h_packed
    }
}

impl std::fmt::Debug for Sizes {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Sizes")
            .field("w", &self.w())
            .field("h", &self.h())
            .finish()
    }
}

impl From<Sizing> for Sizes {
    fn from(s: Sizing) -> Self {
        Self::new(s, s)
    }
}

impl std::hash::Hash for Sizes {
    #[inline]
    fn hash<H: std::hash::Hasher>(&self, h: &mut H) {
        h.write_u64(self.as_u64());
    }
}

impl<T: Num> From<T> for Sizes {
    fn from(v: T) -> Self {
        let s = Sizing::fixed(v.as_f32());
        Self::new(s, s)
    }
}

impl<W: Into<Sizing>, H: Into<Sizing>> From<(W, H)> for Sizes {
    fn from((w, h): (W, H)) -> Self {
        Self::new(w.into(), h.into())
    }
}

impl From<Size> for Sizes {
    fn from(s: Size) -> Self {
        Self::new(Sizing::fixed(s.w), Sizing::fixed(s.h))
    }
}

#[cfg(test)]
mod tests {
    use crate::layout::types::sizing::{Sizes, Sizing};
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    fn hash_value(value: impl Hash) -> u64 {
        let mut hasher = DefaultHasher::new();
        value.hash(&mut hasher);
        hasher.finish()
    }

    #[test]
    fn signed_zero_sizing_and_sizes_share_equality_and_hashes() {
        let positive = Sizing::fixed(0.0);
        let negative = Sizing::fixed(-0.0);
        assert_eq!(positive, negative);
        assert_eq!(hash_value(positive), hash_value(negative));

        let positive = Sizes::new(positive, Sizing::HUG);
        let negative = Sizes::new(negative, Sizing::HUG);
        assert_eq!(positive, negative);
        assert_eq!(hash_value(positive), hash_value(negative));
    }

    #[test]
    fn constructors_accept_only_finite_valid_payloads() {
        assert_eq!(Sizing::fixed(f32::MAX).fixed_value(), Some(f32::MAX));
        assert_eq!(Sizing::fill(f32::MAX).fill_weight(), Some(f32::MAX));
        assert_eq!(Sizing::share(0.0), Sizing::fixed(0.0));
        assert_eq!(Sizing::share(-0.0), Sizing::fixed(0.0));
        assert_eq!(Sizing::share(2.5), Sizing::fill(2.5));

        let smallest_positive = f32::from_bits(1);
        let packed = Sizes::new(Sizing::fill(smallest_positive), Sizing::HUG);
        // Dropping two bits yields 0; the positive floor stores 1, then decode restores bits 4.
        assert_eq!(packed.w().fill_weight(), Some(f32::from_bits(4)));

        type Case = (&'static str, fn() -> Sizing);
        let cases: &[Case] = &[
            ("negative fixed", || Sizing::fixed(-1.0)),
            ("NaN fixed", || Sizing::fixed(f32::NAN)),
            ("positive-infinite fixed", || Sizing::fixed(f32::INFINITY)),
            ("negative-infinite fixed", || {
                Sizing::fixed(f32::NEG_INFINITY)
            }),
            ("zero fill", || Sizing::fill(0.0)),
            ("negative-zero fill", || Sizing::fill(-0.0)),
            ("negative fill", || Sizing::fill(-1.0)),
            ("NaN fill", || Sizing::fill(f32::NAN)),
            ("infinite fill", || Sizing::fill(f32::INFINITY)),
            ("negative share", || Sizing::share(-1.0)),
            ("NaN share", || Sizing::share(f32::NAN)),
            ("infinite share", || Sizing::share(f32::INFINITY)),
        ];
        for &(label, construct) in cases {
            assert!(
                std::panic::catch_unwind(construct).is_err(),
                "case `{label}` must panic",
            );
        }
    }
}
