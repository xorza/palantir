use crate::primitives::approx::noop_f32;
use crate::primitives::brush::Brush;
use crate::primitives::color::Color;
use palantir_anim_derive::Animatable;

/// Stroke paint: brush + width. No longer `Pod` (the user-facing
/// `Brush` is an enum); the renderer's `Quad` carries an inline
/// `stroke_color: Color` + `stroke_width: f32` pair instead and the
/// composer translates between the two.
#[derive(
    Clone, Copy, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize, Animatable,
)]
pub struct Stroke {
    pub brush: Brush,
    pub width: f32,
}

impl Stroke {
    /// Canonical "no stroke" — width 0, transparent brush. Equivalent
    /// to `Stroke::default()` but `const`, so callers can use it in
    /// const contexts and read it as the sentinel "this background
    /// has no stroke" without needing `Option<Stroke>` in the type.
    pub const ZERO: Self = Self {
        brush: Brush::TRANSPARENT,
        width: 0.0,
    };

    /// True when this stroke would paint nothing visible — width is
    /// sub-UI-tolerance (including negative, treated as zero), or
    /// the brush is fully transparent. The animation pipeline lerps
    /// `Stroke` directly through `Stroke::ZERO`, so a "stroked →
    /// no-stroke" transition settles at `is_noop()` and the encoder
    /// filters it out without any `Option` collapse step.
    #[inline]
    pub fn is_noop(&self) -> bool {
        noop_f32(self.width) || self.brush.is_noop()
    }
}

impl Stroke {
    /// Solid-stroke shorthand for the common `Color`-only case. Slice 1
    /// callers pass `color` directly; future gradient/image strokes go
    /// through the struct literal with an explicit `Brush::Linear(...)`.
    #[inline]
    pub const fn solid(color: Color, width: f32) -> Self {
        Self {
            brush: Brush::Solid(color),
            width,
        }
    }
}

impl std::hash::Hash for Stroke {
    #[inline]
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.brush.hash(state);
        state.write_u32(self.width.to_bits());
    }
}
