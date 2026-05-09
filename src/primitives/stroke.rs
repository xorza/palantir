use crate::primitives::approx::approx_zero;
use crate::primitives::color::Color;
use palantir_anim_derive::Animatable;

#[repr(C)]
#[derive(
    Clone,
    Copy,
    Debug,
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
    /// True when this stroke would paint nothing visible — width is
    /// approximately zero (sub-UI-tolerance) or the color is fully
    /// transparent. Used by [`Background::is_noop`] and by animated
    /// "stroked → no-stroke" transitions to collapse a decayed
    /// `Some(Stroke)` to `None` so it doesn't render as a phantom
    /// hairline.
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
