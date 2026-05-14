use crate::primitives::num::Num;
use crate::primitives::size::Size;

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
}

impl<T: Num> From<T> for Sizing {
    fn from(v: T) -> Self {
        Sizing::Fixed(v.as_f32())
    }
}

/// Tagged-union with niche-uninit padding in the inactive variant — raw
/// `bytes_of` would hash junk. Encode `tag:u8 + value:f32` instead.
impl std::hash::Hash for Sizing {
    #[inline]
    fn hash<H: std::hash::Hasher>(&self, h: &mut H) {
        let (tag, v) = match *self {
            Sizing::Fixed(v) => (0u8, v),
            Sizing::Hug => (1, 0.0),
            Sizing::Fill(w) => (2, w),
        };
        h.write_u8(tag);
        h.write_u32(v.to_bits());
    }
}

/// Per-axis `Sizing`. Construct via `Default` (Hug × Hug), `Sizes::from(s)`
/// (uniform), `Sizes::from(n)` (uniform Fixed via `Num`), or
/// `Sizes::from((w, h))` for asymmetric. The `From` impls are the public
/// surface — `Configure::size` takes `impl Into<Sizes>` so call sites stay
/// terse: `.size(100.0)`, `.size(Sizing::FILL)`, `.size((Sizing::FILL, 40.0))`.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Sizes {
    pub w: Sizing,
    pub h: Sizing,
}

impl From<Sizing> for Sizes {
    fn from(s: Sizing) -> Self {
        Self { w: s, h: s }
    }
}

impl std::hash::Hash for Sizes {
    #[inline]
    fn hash<H: std::hash::Hasher>(&self, h: &mut H) {
        self.w.hash(h);
        self.h.hash(h);
    }
}

impl<T: Num> From<T> for Sizes {
    fn from(v: T) -> Self {
        let s = Sizing::Fixed(v.as_f32());
        Self { w: s, h: s }
    }
}

impl<W: Into<Sizing>, H: Into<Sizing>> From<(W, H)> for Sizes {
    fn from((w, h): (W, H)) -> Self {
        Self {
            w: w.into(),
            h: h.into(),
        }
    }
}

impl From<Size> for Sizes {
    fn from(s: Size) -> Self {
        Self {
            w: Sizing::Fixed(s.w),
            h: Sizing::Fixed(s.h),
        }
    }
}
