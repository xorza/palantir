//! What fills a shape: a solid color or a gradient.
//!
//! Colors blend premultiplied wherever they interpolate (gradient stops, mesh vertices,
//! polyline per-point colors, join averages), per CSS Color 4 §12.3: opaque white fading to
//! transparent black is white at half alpha midway, not darkened.

pub(crate) mod gradient;

use crate::primitives::math::animatable::Animatable;
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
    /// One color.
    Solid(RgbaF32),
    /// Linear gradient.
    Linear(LinearGradient),
    /// Radial gradient.
    Radial(RadialGradient),
    /// Conic gradient.
    Conic(ConicGradient),
}

impl Brush {
    /// Panics unless the color or gradient geometry holds its kinds ([`domain`]); stops are checked when made.
    #[inline]
    #[track_caller]
    pub(crate) const fn validate(&self) {
        match self {
            Self::Solid(c) => {
                let _ = domain::color(*c);
            }
            Self::Linear(g) => {
                domain::angle(g.geometry.angle);
            }
            Self::Radial(g) => {
                vec2::offset(g.geometry.center);
                vec2::length(g.geometry.radius);
            }
            Self::Conic(g) => {
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
            Self::Solid(c) => c.is_noop(),
            Self::Linear(g) => g.is_noop(),
            Self::Radial(g) => g.is_noop(),
            Self::Conic(g) => g.is_noop(),
        }
    }

    #[inline]
    /// The color, if solid.
    pub const fn as_solid(&self) -> Option<RgbaF32> {
        match self {
            Self::Solid(c) => Some(*c),
            Self::Linear(_) | Self::Radial(_) | Self::Conic(_) => None,
        }
    }
}

impl Default for Brush {
    #[inline]
    fn default() -> Self {
        Self::TRANSPARENT
    }
}

impl From<RgbaF32> for Brush {
    #[inline]
    fn from(c: RgbaF32) -> Self {
        Self::Solid(c)
    }
}

impl From<SrgbaU8> for Brush {
    #[inline]
    fn from(color: SrgbaU8) -> Self {
        Self::Solid(color.into())
    }
}

impl From<LinearGradient> for Brush {
    #[inline]
    fn from(gradient: LinearGradient) -> Self {
        Self::Linear(gradient)
    }
}

impl From<LinearGradientBuilder> for Brush {
    #[inline]
    fn from(builder: LinearGradientBuilder) -> Self {
        Self::Linear(builder.build())
    }
}

impl From<RadialGradient> for Brush {
    #[inline]
    fn from(gradient: RadialGradient) -> Self {
        Self::Radial(gradient)
    }
}

impl From<RadialGradientBuilder> for Brush {
    #[inline]
    fn from(builder: RadialGradientBuilder) -> Self {
        Self::Radial(builder.build())
    }
}

impl From<ConicGradient> for Brush {
    #[inline]
    fn from(gradient: ConicGradient) -> Self {
        Self::Conic(gradient)
    }
}

impl From<ConicGradientBuilder> for Brush {
    #[inline]
    fn from(builder: ConicGradientBuilder) -> Self {
        Self::Conic(builder.build())
    }
}

impl Animatable for Brush {
    #[inline]
    fn lerp(a: Self, b: Self, t: f32) -> Self {
        // Match by reference so the gradient fallback can return an original without cloning.
        match (&a, &b) {
            (Self::Solid(x), Self::Solid(y)) => Self::Solid(RgbaF32::lerp(*x, *y, t)),
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
            (Self::Solid(x), Self::Solid(y)) => Self::Solid(x.sub(*y)),
            _ => Self::zero(),
        }
    }

    #[inline]
    fn add(self, other: Self) -> Self {
        match (&self, &other) {
            (Self::Solid(x), Self::Solid(y)) => Self::Solid(x.add(*y)),
            _ => self,
        }
    }

    #[inline]
    fn scale(self, k: f32) -> Self {
        match self {
            Self::Solid(c) => Self::Solid(c.scale(k)),
            Self::Linear(_) | Self::Radial(_) | Self::Conic(_) => Self::zero(),
        }
    }

    #[inline]
    fn magnitude_squared(self) -> f32 {
        match self {
            Self::Solid(c) => c.magnitude_squared(),
            Self::Linear(_) | Self::Radial(_) | Self::Conic(_) => 0.0,
        }
    }

    #[inline]
    fn settle_distance_squared(self) -> f32 {
        match self {
            Self::Solid(c) => c.settle_distance_squared(),
            Self::Linear(_) | Self::Radial(_) | Self::Conic(_) => 0.0,
        }
    }

    #[inline]
    fn zero() -> Self {
        Self::Solid(RgbaF32::zero())
    }

    #[inline]
    fn to_motion(self) -> Self {
        match self {
            Self::Solid(c) => Self::Solid(c.to_motion()),
            gradient @ (Self::Linear(_) | Self::Radial(_) | Self::Conic(_)) => gradient,
        }
    }

    #[inline]
    fn from_motion(motion: Self) -> Self {
        match motion {
            Self::Solid(c) => Self::Solid(RgbaF32::from_motion(c)),
            gradient @ (Self::Linear(_) | Self::Radial(_) | Self::Conic(_)) => gradient,
        }
    }

    #[inline]
    fn normalize_for_spring(&mut self, target: &Self, velocity: &mut Self) {
        if !matches!((&*self, target), (Self::Solid(_), Self::Solid(_))) {
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
