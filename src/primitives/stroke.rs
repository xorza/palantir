use crate::primitives::approx::approx_zero;
use crate::primitives::color::Color;
use palantir_anim_derive::Animatable;

#[repr(C)]
#[derive(
    Clone,
    Copy,
    Debug,
    Default,
    PartialEq,
    bytemuck::Pod,
    bytemuck::Zeroable,
    serde::Serialize,
    serde::Deserialize,
    Animatable,
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
    /// approximately zero (sub-UI-tolerance) or the color is fully
    /// transparent. The animation pipeline lerps `Stroke` directly
    /// through `Stroke::ZERO`, so a "stroked → no-stroke" transition
    /// settles at `is_noop()` and the encoder filters it out without
    /// any `Option` collapse step.
    #[inline]
    pub fn is_noop(&self) -> bool {
        approx_zero(self.width) || self.color.is_noop()
    }
}

impl std::hash::Hash for Stroke {
    #[inline]
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        state.write(bytemuck::bytes_of(self));
    }
}
