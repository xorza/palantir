use crate::primitives::color::Color;

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
)]
pub struct Stroke {
    pub color: Color,
    pub width: f32,
}

impl std::hash::Hash for Stroke {
    #[inline]
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        state.write(bytemuck::bytes_of(self));
    }
}
