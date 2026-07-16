use crate::primitives::{approx, num::Num, size::Size};

/// WPF-style sizing. Maps to: Fixed = exact px, Hug = Auto (use desired),
/// Fill = Star (take remainder, distributed by `weight` across Fill siblings).
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub enum Sizing {
    Fixed(f32),
    #[default]
    Hug,
    Fill(f32),
}

impl Sizing {
    /// Equal-weight `Fill`. Equivalent to `Sizing::Fill(1.0)`.
    pub const FILL: Self = Self::Fill(1.0);

    /// Panic if the embedded value is out of range. `Sizing::Fixed` is a
    /// pixel extent (must be ≥ 0). `Sizing::Fill` is a relative weight; a
    /// zero weight has no useful semantics — Stack would silently collapse
    /// such a child to zero width when sharing leftover with positive-weight
    /// siblings, and Grid filters it out of the Fill pool — so reject it
    /// here. `Hug` carries no value.
    pub const fn assert_non_negative(self) {
        match self {
            Sizing::Fixed(v) => assert!(v >= 0.0, "Sizing::Fixed must be non-negative"),
            Sizing::Fill(w) => assert!(w > 0.0, "Sizing::Fill weight must be positive"),
            Sizing::Hug => {}
        }
    }

    /// Debug-only variant for the builder hot path — `Configure::size`
    /// runs per widget per frame and the const `assert_non_negative`
    /// shows up at ~0.8% self time in release. `Track::new` (a const
    /// fn) still uses the asserting variant for compile-time checks.
    #[inline]
    pub(crate) fn debug_assert_non_negative(self) {
        debug_assert!(
            match self {
                Sizing::Fixed(v) => v >= 0.0,
                Sizing::Fill(w) => w > 0.0,
                Sizing::Hug => true,
            },
            "Sizing out of range: {self:?}",
        );
    }

    #[inline]
    pub(crate) fn hash_visual<H: std::hash::Hasher>(&self, h: &mut H) {
        let (tag, value) = match *self {
            Sizing::Fixed(value) => (0u8, value),
            Sizing::Hug => (1, 0.0),
            Sizing::Fill(value) => (2, value),
        };
        h.write_u64((tag as u64) | ((approx::canon_bits(value) as u64) << 8));
    }
}

impl<T: Num> From<T> for Sizing {
    fn from(v: T) -> Self {
        Sizing::Fixed(v.as_f32())
    }
}

/// Tagged-union with niche-uninit padding in the inactive variant — raw
/// `bytes_of` would hash junk. Encode `tag:u8 + value:f32` into one
/// `u64` write (tag low, value bits high 32) instead of two small calls.
impl std::hash::Hash for Sizing {
    #[inline]
    fn hash<H: std::hash::Hasher>(&self, h: &mut H) {
        let (tag, v) = match *self {
            Sizing::Fixed(v) => (0u8, v),
            Sizing::Hug => (1, 0.0),
            Sizing::Fill(w) => (2, w),
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
/// Read components via `Sizes::w()` / `Sizes::h()` — they return a
/// fresh `Sizing` enum so pattern matching at use sites is unchanged.
#[derive(Clone, Copy)]
pub struct Sizes {
    w_packed: u32,
    h_packed: u32,
}

impl Default for Sizes {
    #[inline]
    fn default() -> Self {
        Self::new(Sizing::Hug, Sizing::Hug)
    }
}

const SIZING_TAG_FIXED: u32 = 0;
const SIZING_TAG_HUG: u32 = 1;
const SIZING_TAG_FILL: u32 = 2;
const SIZING_TAG_SHIFT: u32 = 30;
const SIZING_VAL_MASK: u32 = (1 << 30) - 1;

#[inline]
const fn encode_sizing(s: Sizing) -> u32 {
    let (tag, v) = match s {
        Sizing::Fixed(v) => (SIZING_TAG_FIXED, v),
        Sizing::Hug => (SIZING_TAG_HUG, 0.0),
        Sizing::Fill(w) => (SIZING_TAG_FILL, w),
    };
    (tag << SIZING_TAG_SHIFT) | (approx::eq_bits(v) >> 2)
}

#[inline]
const fn decode_sizing(packed: u32) -> Sizing {
    let tag = packed >> SIZING_TAG_SHIFT;
    let val = f32::from_bits((packed & SIZING_VAL_MASK) << 2);
    match tag {
        SIZING_TAG_FIXED => Sizing::Fixed(val),
        SIZING_TAG_HUG => Sizing::Hug,
        SIZING_TAG_FILL => Sizing::Fill(val),
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
        let s = Sizing::Fixed(v.as_f32());
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
        Self::new(Sizing::Fixed(s.w), Sizing::Fixed(s.h))
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
        let positive = Sizing::Fixed(0.0);
        let negative = Sizing::Fixed(-0.0);
        assert_eq!(positive, negative);
        assert_eq!(hash_value(positive), hash_value(negative));

        let positive = Sizes::new(positive, Sizing::Hug);
        let negative = Sizes::new(negative, Sizing::Hug);
        assert_eq!(positive, negative);
        assert_eq!(hash_value(positive), hash_value(negative));
    }
}
