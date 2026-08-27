pub(crate) mod gradient;

use crate::animation::animatable::Animatable;
use crate::primitives::brush::gradient::conic_geometry::{ConicGradient, ConicGradientBuilder};
use crate::primitives::brush::gradient::linear_geometry::{LinearGradient, LinearGradientBuilder};
use crate::primitives::brush::gradient::radial_geometry::{RadialGradient, RadialGradientBuilder};
use crate::primitives::color::{Color, ColorU8};
use crate::primitives::nan::NanCheck;

/// Paint source for gradient-capable fills.
///
/// `Solid(Color)` is the hot 99% path — 16 B inline, animation-lerpable.
/// `Linear`/`Radial`/`Conic` carry their geometry plus a
/// [`GradientStops`](crate::GradientStops) array inline, which
/// is what sizes the whole enum; gradient morph animations snap across
/// variants and across distinct gradients of the same variant.
// `Brush` is intentionally **not `Copy`** — the gradient variants carry
// 40 B of inline stops, putting the enum at 60 B (pinned by
// `hot_struct_sizes_are_pinned`). The recording chain threads `Brush`
// (usually inside `Background`) through three or four functions per
// chromed widget, where auto-`Copy` hides a `vmovups` per hop per
// frame in the node opener. Hot paths pass `&Brush` / `&Background`;
// explicit `.clone()` at the remaining duplication sites keeps the cost
// auditable. See `Animatable`'s `Clone` (not `Copy`) supertrait for the
// matching animation-side relaxation.
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
///
/// A [`Brush`] behind a narrower door rather than a second enum beside it:
/// the four `From` impls below are the only way to build one, so the
/// radial and conic variants are unreachable by construction. It answers
/// nothing itself — [`Self::as_brush`] hands the `Brush` back, and the
/// no-op test, the NaN screen and the lowering stay single-sourced there.
#[derive(Clone, Debug, PartialEq)]
pub struct CurveBrush(Brush);

impl CurveBrush {
    pub const TRANSPARENT: Self = Self(Brush::TRANSPARENT);

    /// The paint source, for consumers that read, screen or lower it.
    #[inline]
    pub const fn as_brush(&self) -> &Brush {
        &self.0
    }
}

impl From<Color> for CurveBrush {
    #[inline]
    fn from(color: Color) -> Self {
        Self(Brush::from(color))
    }
}

impl From<ColorU8> for CurveBrush {
    #[inline]
    fn from(color: ColorU8) -> Self {
        Self(Brush::from(color))
    }
}

impl From<LinearGradient> for CurveBrush {
    #[inline]
    fn from(gradient: LinearGradient) -> Self {
        Self(Brush::from(gradient))
    }
}

impl From<LinearGradientBuilder> for CurveBrush {
    #[inline]
    fn from(builder: LinearGradientBuilder) -> Self {
        Self(Brush::from(builder))
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

    /// Extracts the linear gradient, the one kind a [`CurveBrush`] can
    /// also hold. Returns `None` for every other variant.
    #[inline]
    pub const fn as_linear(&self) -> Option<&LinearGradient> {
        match self {
            Brush::Linear(g) => Some(g),
            Brush::Solid(_) | Brush::Radial(_) | Brush::Conic(_) => None,
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
        // re-`Clone` — the tuple-by-value pattern needs `Brush: Copy`,
        // and the trait requires only `Clone`.
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

impl NanCheck for Brush {
    #[inline]
    fn has_nan(&self) -> bool {
        match self {
            Self::Solid(color) => color.has_nan(),
            Self::Linear(gradient) => gradient.has_nan(),
            Self::Radial(gradient) => gradient.has_nan(),
            Self::Conic(gradient) => gradient.has_nan(),
        }
    }
}

#[cfg(test)]
mod tests;
