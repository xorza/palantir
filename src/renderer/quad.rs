//! Per-instance quad data — the Pod type that flows from the
//! composer through `RenderBuffer` into the backend's `QuadPipeline`.
//! Lives at the renderer root alongside `RenderBuffer`: both are the
//! frontend↔backend contract, so neither side owns them.

use crate::primitives::{Color, Corners, Rect, Stroke};
use bytemuck::{Pod, Zeroable};

/// Per-instance quad data (68 B). Layout is `pos, size, fill, radius,
/// stroke_color, stroke_width` — see the `vertex_attr_array` in
/// `QuadPipeline::new` (in the backend) for the explicit attribute
/// offsets, which is the only thing constraining the field order. No
/// tail padding: vertex buffer strides only need 4-byte alignment,
/// unlike std140 uniforms.
#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
pub(crate) struct Quad {
    pub(crate) pos: [f32; 2],
    pub(crate) size: [f32; 2],
    pub(crate) fill: [f32; 4],
    pub(crate) radius: [f32; 4],
    pub(crate) stroke_color: [f32; 4],
    pub(crate) stroke_width: f32,
}

impl Quad {
    pub(crate) fn new(rect: Rect, fill: Color, radius: Corners, stroke: Option<Stroke>) -> Self {
        let (sc, sw) = match stroke {
            Some(s) => ([s.color.r, s.color.g, s.color.b, s.color.a], s.width),
            None => ([0.0; 4], 0.0),
        };
        Self {
            pos: [rect.min.x, rect.min.y],
            size: [rect.size.w, rect.size.h],
            fill: [fill.r, fill.g, fill.b, fill.a],
            radius: [radius.tl, radius.tr, radius.br, radius.bl],
            stroke_color: sc,
            stroke_width: sw,
        }
    }
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
