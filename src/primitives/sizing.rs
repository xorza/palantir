use super::Num;

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

    /// Panic if the embedded value is negative. `Sizing::Fixed` is a pixel
    /// extent and `Sizing::Fill` is a relative weight — neither is meaningful
    /// below zero. `Hug` carries no value.
    pub const fn assert_non_negative(self) {
        match self {
            Sizing::Fixed(v) => assert!(v >= 0.0, "Sizing::Fixed must be non-negative"),
            Sizing::Fill(w) => assert!(w >= 0.0, "Sizing::Fill weight must be non-negative"),
            Sizing::Hug => {}
        }
    }
}

impl<T: Num> From<T> for Sizing {
    fn from(v: T) -> Self {
        Sizing::Fixed(v.as_f32())
    }
}

/// Per-axis `Sizing`. Construct via `Default` (Hug × Hug), `Sizes::from(s)`
/// (uniform), `Sizes::from(n)` (uniform Fixed via `Num`), or
/// `Sizes::from((w, h))` for asymmetric. The `From` impls are the public
/// surface — `Element::size` takes `impl Into<Sizes>` so call sites stay
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
