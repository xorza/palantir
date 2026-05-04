use crate::primitives::color::Color;

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Stroke {
    pub color: Color,
    pub width: f32,
}
