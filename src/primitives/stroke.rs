use crate::primitives::color::Color;

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Stroke {
    pub width: f32,
    pub color: Color,
}
