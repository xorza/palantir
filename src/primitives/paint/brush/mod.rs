//! What fills a shape: a solid colour or a gradient.
//!
//! Colours blend premultiplied wherever they interpolate (gradient stops, mesh vertices,
//! polyline per-point colours, join averages), per CSS Color 4 §12.3: opaque white fading to
//! transparent black is white at half alpha midway, not darkened.

pub(crate) mod gradient;

use crate::animation::animatable::Animatable;
use crate::primitives::math::domain::{self, vec2};
use crate::primitives::math::nan::NanCheck;
use crate::primitives::paint::brush::gradient::conic_geometry::{
    ConicGradient, ConicGradientBuilder,
};
use crate::primitives::paint::brush::gradient::linear_geometry::{
    LinearGradient, LinearGradientBuilder,
};
use crate::primitives::paint::brush::gradient::radial_geometry::{
    RadialGradient, RadialGradientBuilder,
};
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::color::srgba_u8::SrgbaU8;

/// Paint source for gradient-capable fills; gradient morphs snap across variants.
// Not `Copy`: the gradient variants carry inline stops and the recording chain threads a brush
// through several functions per widget; see `Animatable`.
#[derive(Clone, Debug, PartialEq, ::serde::Serialize, ::serde::Deserialize)]
pub enum Brush {
    /// One colour.
    Solid(RgbaF32),
    /// Linear gradient.
    Linear(LinearGradient),
    /// Radial gradient.
    Radial(RadialGradient),
    /// Conic gradient.
    Conic(ConicGradient),
}

impl Brush {
    /// Panics unless the colour or gradient geometry holds its kinds ([`domain`]); stops are checked when made.
    #[inline]
    #[track_caller]
    pub(crate) const fn validate(&self) {
        match self {
            Brush::Solid(c) => {
                let _ = domain::color(*c);
            }
            Brush::Linear(g) => {
                domain::angle(g.geometry.angle);
            }
            Brush::Radial(g) => {
                vec2::offset(g.geometry.center);
                vec2::length(g.geometry.radius);
            }
            Brush::Conic(g) => {
                vec2::offset(g.geometry.center);
                domain::angle(g.geometry.start_angle);
            }
        }
    }

    /// Paints nothing.
    pub const TRANSPARENT: Self = Self::Solid(RgbaF32::TRANSPARENT);

    #[inline]
    /// Whether the brush paints nothing.
    pub const fn is_noop(&self) -> bool {
        match self {
            Brush::Solid(c) => c.is_noop(),
            Brush::Linear(g) => g.is_noop(),
            Brush::Radial(g) => g.is_noop(),
            Brush::Conic(g) => g.is_noop(),
        }
    }

    #[inline]
    /// The colour, if solid.
    pub const fn as_solid(&self) -> Option<RgbaF32> {
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

impl From<RgbaF32> for Brush {
    #[inline]
    fn from(c: RgbaF32) -> Self {
        Brush::Solid(c)
    }
}

impl From<SrgbaU8> for Brush {
    #[inline]
    fn from(color: SrgbaU8) -> Self {
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

impl Animatable for Brush {
    #[inline]
    fn lerp(a: Self, b: Self, t: f32) -> Self {
        // Match by reference so the gradient fallback can return an original without cloning.
        match (&a, &b) {
            (Brush::Solid(x), Brush::Solid(y)) => Brush::Solid(RgbaF32::lerp(*x, *y, t)),
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
    fn settle_distance_squared(self) -> f32 {
        match self {
            Brush::Solid(c) => c.settle_distance_squared(),
            Brush::Linear(_) | Brush::Radial(_) | Brush::Conic(_) => 0.0,
        }
    }

    #[inline]
    fn zero() -> Self {
        Brush::Solid(RgbaF32::zero())
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
