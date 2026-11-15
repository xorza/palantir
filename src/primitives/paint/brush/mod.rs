//! What fills a shape: a solid color or a gradient.
//!
//! Colors blend premultiplied wherever they interpolate (gradient stops, mesh vertices,
//! polyline per-point colors, join averages), per CSS Color 4 §12.3: opaque white fading to
//! transparent black is white at half alpha midway, not darkened.

pub(crate) mod gradient;

use crate::primitives::math::animatable::Animatable;
use crate::primitives::math::domain;
use crate::primitives::math::lerp::Lerp;
use crate::primitives::math::nan::NanCheck;
use crate::primitives::paint::brush::gradient::Gradient;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::color::srgba_u8::SrgbaU8;

/// Paint source for gradient-capable fills; gradient morphs snap across variants.
// Not `Copy`: the gradient variants carry inline stops and the recording chain threads a brush
// through several functions per widget; see `Animatable`.
#[derive(Clone, Debug, PartialEq, ::serde::Serialize, ::serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub enum Brush {
    /// One color.
    Solid(RgbaF32),
    /// A gradient of any kind.
    Gradient(Gradient),
}

impl Brush {
    /// Whether the color or gradient geometry holds its kinds: [`Self::validate`]'s test.
    #[inline]
    pub(crate) const fn is_valid(&self) -> bool {
        match self {
            Self::Solid(c) => domain::is_color(*c),
            Self::Gradient(g) => g.geometry.is_valid(),
        }
    }

    /// Panics unless the color or gradient geometry holds its kinds ([`domain`]); stops are
    /// checked when made.
    #[inline]
    #[track_caller]
    pub(crate) const fn validate(&self) {
        match self {
            Self::Solid(c) => {
                let _ = domain::color(*c);
            }
            Self::Gradient(g) => g.geometry.validate(),
        }
    }

    /// Paints nothing.
    pub const TRANSPARENT: Self = Self::Solid(RgbaF32::TRANSPARENT);

    #[inline]
    /// Whether the brush paints nothing.
    pub const fn is_noop(&self) -> bool {
        match self {
            Self::Solid(c) => c.is_noop(),
            Self::Gradient(g) => g.is_noop(),
        }
    }

    #[inline]
    /// The color, if solid.
    pub const fn as_solid(&self) -> Option<RgbaF32> {
        match self {
            Self::Solid(c) => Some(*c),
            Self::Gradient(_) => None,
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

impl From<Gradient> for Brush {
    #[inline]
    fn from(gradient: Gradient) -> Self {
        Self::Gradient(gradient)
    }
}

impl Lerp for Brush {
    #[inline]
    fn lerp(a: &Self, b: &Self, t: f32) -> Self {
        match (a, b) {
            (Self::Solid(x), Self::Solid(y)) => Self::Solid(RgbaF32::lerp(x, y, t)),
            // No blend reaches a gradient yet, so it switches halfway, as a snap field does.
            _ => {
                if t < 0.5 {
                    a.clone()
                } else {
                    b.clone()
                }
            }
        }
    }
}

impl Animatable for Brush {
    #[inline]
    fn sub(&self, other: &Self) -> Self {
        match (self, other) {
            (Self::Solid(x), Self::Solid(y)) => Self::Solid(x.sub(y)),
            _ => Self::zero(),
        }
    }

    #[inline]
    fn add(&self, other: &Self) -> Self {
        match (self, other) {
            (Self::Solid(x), Self::Solid(y)) => Self::Solid(x.add(y)),
            _ => self.clone(),
        }
    }

    #[inline]
    fn scale(&self, k: f32) -> Self {
        match self {
            Self::Solid(c) => Self::Solid(c.scale(k)),
            Self::Gradient(_) => Self::zero(),
        }
    }

    #[inline]
    fn magnitude_squared(&self) -> f32 {
        match self {
            Self::Solid(c) => c.magnitude_squared(),
            Self::Gradient(_) => 0.0,
        }
    }

    #[inline]
    fn settle_distance_squared(&self) -> f32 {
        match self {
            Self::Solid(c) => c.settle_distance_squared(),
            Self::Gradient(_) => 0.0,
        }
    }

    #[inline]
    fn zero() -> Self {
        Self::Solid(RgbaF32::zero())
    }

    #[inline]
    fn to_motion(&self) -> Self {
        match self {
            Self::Solid(c) => Self::Solid(c.to_motion()),
            gradient @ Self::Gradient(_) => gradient.clone(),
        }
    }

    #[inline]
    fn from_motion(motion: Self) -> Self {
        match motion {
            Self::Solid(c) => Self::Solid(RgbaF32::from_motion(c)),
            gradient @ Self::Gradient(_) => gradient,
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
            Self::Gradient(gradient) => gradient.has_nan(),
        }
    }
}

#[cfg(test)]
mod tests;
