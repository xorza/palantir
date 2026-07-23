pub(crate) mod gradient;

use crate::animation::animatable::Animatable;
use crate::primitives::brush::gradient::conic::{ConicGradient, ConicGradientBuilder};
use crate::primitives::brush::gradient::linear::{LinearGradient, LinearGradientBuilder};
use crate::primitives::brush::gradient::radial::{RadialGradient, RadialGradientBuilder};
use crate::primitives::color::{Color, ColorU8};

/// Paint source for gradient-capable fills.
///
/// `Solid(Color)` is the hot 99% path — 16 B inline, animation-lerpable.
/// `Linear`/`Radial`/`Conic` carry their geometry inline (~80 B);
/// gradient morph animations snap across variants and across distinct
/// gradients of the same variant.
// `Brush` is intentionally **not `Copy`** — the gradient variants
// carry 40 B of inline stops and the whole enum is 60 B. The
// recording chain used to thread `Brush` (often inside `Background`)
// through 3-4 functions per chromed widget by value; auto-`Copy` hid
// an O(N) of `vmovups` per frame in `Ui::node`. Hot paths
// now pass `&Brush` / `&Background`; explicit `.clone()` at the
// remaining duplication sites keeps the cost auditable. See
// `Animatable`'s `Clone` (not `Copy`) supertrait for the matching
// animation-side relaxation.
#[derive(Clone, Debug, PartialEq, ::serde::Serialize, ::serde::Deserialize)]
pub enum Brush {
    Solid(Color),
    Linear(LinearGradient),
    Radial(RadialGradient),
    Conic(ConicGradient),
}

/// Paint source for one-dimensional stroked shapes. Solid colors and linear
/// gradients have an unambiguous mapping along the curve parameter; radial and
/// conic gradients do not.
#[derive(Clone, Debug, PartialEq)]
pub enum CurveBrush {
    Solid(Color),
    Linear(LinearGradient),
}

impl CurveBrush {
    pub(crate) const TRANSPARENT: Self = Self::Solid(Color::TRANSPARENT);

    #[inline]
    pub(crate) fn is_noop(&self) -> bool {
        match self {
            CurveBrush::Solid(color) => color.is_noop(),
            CurveBrush::Linear(gradient) => gradient.is_noop(),
        }
    }
}

impl From<Color> for CurveBrush {
    #[inline]
    fn from(color: Color) -> Self {
        CurveBrush::Solid(color)
    }
}

impl From<ColorU8> for CurveBrush {
    #[inline]
    fn from(color: ColorU8) -> Self {
        CurveBrush::Solid(color.into())
    }
}

impl From<LinearGradient> for CurveBrush {
    #[inline]
    fn from(gradient: LinearGradient) -> Self {
        CurveBrush::Linear(gradient)
    }
}

impl From<LinearGradientBuilder> for CurveBrush {
    #[inline]
    fn from(builder: LinearGradientBuilder) -> Self {
        CurveBrush::Linear(builder.build())
    }
}

impl Brush {
    pub const TRANSPARENT: Self = Self::Solid(Color::TRANSPARENT);

    /// Paints nothing visible.
    #[inline]
    pub fn is_noop(&self) -> bool {
        match self {
            Brush::Solid(c) => c.is_noop(),
            Brush::Linear(g) => g.is_noop(),
            Brush::Radial(g) => g.is_noop(),
            Brush::Conic(g) => g.is_noop(),
        }
    }

    /// Extracts the underlying `Color` for the solid fast path. Returns
    /// `None` for gradient variants. Takes `&self` so callers with a borrowed
    /// `Brush` don't need to clone just to pull out the solid color.
    #[inline]
    pub const fn as_solid(&self) -> Option<Color> {
        match self {
            Brush::Solid(c) => Some(*c),
            Brush::Linear(_) | Brush::Radial(_) | Brush::Conic(_) => None,
        }
    }
}

impl Default for Brush {
    #[inline]
    fn default() -> Self {
        Brush::TRANSPARENT
    }
}

impl From<Color> for Brush {
    #[inline]
    fn from(c: Color) -> Self {
        Brush::Solid(c)
    }
}

impl From<ColorU8> for Brush {
    #[inline]
    fn from(color: ColorU8) -> Self {
        Brush::Solid(color.into())
    }
}

impl From<LinearGradient> for Brush {
    #[inline]
    fn from(gradient: LinearGradient) -> Self {
        Brush::Linear(gradient)
    }
}

impl From<LinearGradientBuilder> for Brush {
    #[inline]
    fn from(builder: LinearGradientBuilder) -> Self {
        Brush::Linear(builder.build())
    }
}

impl From<RadialGradient> for Brush {
    #[inline]
    fn from(gradient: RadialGradient) -> Self {
        Brush::Radial(gradient)
    }
}

impl From<RadialGradientBuilder> for Brush {
    #[inline]
    fn from(builder: RadialGradientBuilder) -> Self {
        Brush::Radial(builder.build())
    }
}

impl From<ConicGradient> for Brush {
    #[inline]
    fn from(gradient: ConicGradient) -> Self {
        Brush::Conic(gradient)
    }
}

impl From<ConicGradientBuilder> for Brush {
    #[inline]
    fn from(builder: ConicGradientBuilder) -> Self {
        Brush::Conic(builder.build())
    }
}

impl std::hash::Hash for Brush {
    #[inline]
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        match self {
            Brush::Solid(c) => {
                state.write_u8(0);
                c.hash(state);
            }
            Brush::Linear(g) => {
                state.write_u8(1);
                g.hash(state);
            }
            Brush::Radial(g) => {
                state.write_u8(2);
                g.hash(state);
            }
            Brush::Conic(g) => {
                state.write_u8(3);
                g.hash(state);
            }
        }
    }
}

impl Animatable for Brush {
    #[inline]
    fn lerp(a: Self, b: Self, t: f32) -> Self {
        // Match on `(&a, &b)` instead of `(a, b)` so the gradient
        // fallback can still hand back one of the originals without
        // re-`Clone` — the tuple-by-value pattern used to work via
        // `Brush: Copy`, but the trait now requires only `Clone`.
        match (&a, &b) {
            (Brush::Solid(x), Brush::Solid(y)) => Brush::Solid(Color::lerp(*x, *y, t)),
            // Gradient morphs snap until interpolation between gradient payloads exists.
            _ => {
                if t >= 1.0 {
                    b
                } else {
                    a
                }
            }
        }
    }

    #[inline]
    fn sub(self, other: Self) -> Self {
        match (&self, &other) {
            (Brush::Solid(x), Brush::Solid(y)) => Brush::Solid(x.sub(*y)),
            _ => Self::zero(),
        }
    }

    #[inline]
    fn add(self, other: Self) -> Self {
        match (&self, &other) {
            (Brush::Solid(x), Brush::Solid(y)) => Brush::Solid(x.add(*y)),
            _ => self,
        }
    }

    #[inline]
    fn scale(self, k: f32) -> Self {
        match self {
            Brush::Solid(c) => Brush::Solid(c.scale(k)),
            Brush::Linear(_) | Brush::Radial(_) | Brush::Conic(_) => Self::zero(),
        }
    }

    #[inline]
    fn magnitude_squared(self) -> f32 {
        match self {
            Brush::Solid(c) => c.magnitude_squared(),
            Brush::Linear(_) | Brush::Radial(_) | Brush::Conic(_) => 0.0,
        }
    }

    #[inline]
    fn zero() -> Self {
        Brush::Solid(Color::zero())
    }

    #[inline]
    fn normalize_for_spring(&mut self, target: &Self, velocity: &mut Self) {
        if !matches!((&*self, target), (Brush::Solid(_), Brush::Solid(_))) {
            if self != target {
                *self = target.clone();
            }
            *velocity = Self::zero();
        }
    }
}

#[cfg(test)]
mod tests;
