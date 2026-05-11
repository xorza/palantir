//! Per-instance quad data — the Pod type that flows from the
//! composer through `RenderBuffer` into the backend's `QuadPipeline`.
//! Lives at the renderer root alongside `RenderBuffer`: both are the
//! frontend↔backend contract, so neither side owns them.

use crate::primitives::{color::Color, corners::Corners, rect::Rect};
use bytemuck::{Pod, Zeroable};

/// Per-instance quad data (68 B). Field types are the matching
/// `repr(C)` primitives, byte-identical to `[f32; N]`s — see the
/// `vertex_attr_array` in `QuadPipeline::new` (in the backend) for the
/// explicit attribute offsets, which is the only thing constraining
/// the field order. No tail padding: vertex buffer strides only need
/// 4-byte alignment, unlike std140 uniforms.
///
/// Stroke is stored as two inline fields (`stroke_color`,
/// `stroke_width`) rather than an embedded `Stroke` so the user-facing
/// `Stroke` is free to carry non-`Pod` paint sources (`Brush`); the
/// composer translates the user `Stroke` into these inline GPU fields.
#[repr(C)]
#[derive(Copy, Clone, Debug, Default, Pod, Zeroable)]
pub(crate) struct Quad {
    pub(crate) rect: Rect,
    pub(crate) fill: Color,
    pub(crate) radius: Corners,
    pub(crate) stroke_color: Color,
    pub(crate) stroke_width: f32,
}

#[cfg(test)]
mod tests {
    use super::Quad;

    /// Pin: `Quad` is exactly 68 bytes — pos(8) + size(8) + fill(16) +
    /// radius(16) + stroke_color(16) + stroke_width(4). The
    /// `vertex_attr_array` in the backend's `QuadPipeline::new` assumes
    /// this exact layout via Rust's `repr(C)` field-order rules. A
    /// reorder or an added field that shifts an attribute's offset
    /// would break the shader binding silently — this test catches it.
    #[test]
    fn quad_struct_is_68_bytes_no_padding() {
        assert_eq!(std::mem::size_of::<Quad>(), 68);
    }
}
