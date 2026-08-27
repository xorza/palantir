//! A shape's outline: one colour and one width.

use crate::primitives::approx::FloatHash;
use crate::primitives::approx::noop_f32;
use crate::primitives::color::Color;
use crate::primitives::nan::NanCheck;
use palantir_anim_derive::Animatable;

/// Solid stroke paint.
#[derive(
    Clone, Copy, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize, Animatable,
)]
pub struct Stroke {
    pub color: Color,
    pub width: f32,
}

impl Stroke {
    /// Canonical "no stroke" — width 0, transparent color. Equivalent
    /// to `Stroke::default()` but `const`, so callers can use it in
    /// const contexts and read it as the sentinel "this background
    /// has no stroke" without needing `Option<Stroke>` in the type.
    pub const ZERO: Self = Self {
        color: Color::TRANSPARENT,
        width: 0.0,
    };

    /// True when this stroke would paint nothing visible — width is
    /// sub-UI-tolerance (including negative, treated as zero), or
    /// the color is fully transparent. The animation pipeline lerps
    /// `Stroke` directly through `Stroke::ZERO`, so a "stroked →
    /// no-stroke" transition settles at `is_noop()` and the encoder
    /// filters it out without any `Option` collapse step.
    /// `&self` where the crate's other `Copy` paint predicates take
    /// `self`: `Background`'s `skip_serializing_if` names this, and
    /// serde requires an `fn(&T) -> bool` there. `Corners::approx_zero`
    /// takes `&self` for the same reason.
    #[inline]
    pub const fn is_noop(&self) -> bool {
        noop_f32(self.width) || self.color.is_noop()
    }

    /// Construct a stroke with `color` and `width`.
    #[inline]
    pub const fn solid(color: Color, width: f32) -> Self {
        Self { color, width }
    }
}

/// Visual throughout: this feeds content-cache keys, so every scalar the
/// stroke carries is canonicalized the same way — see [`FloatHash`].
impl std::hash::Hash for Stroke {
    #[inline]
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.color.hash_visual(state);
        self.width.hash_visual(state);
    }
}
impl NanCheck for Stroke {
    #[inline]
    fn has_nan(&self) -> bool {
        self.color.has_nan() || self.width.is_nan()
    }
}
