//! The [`ViewportPush`] carrier for `imm.viewport_size` and `imm.frag_origin`, the whole of the
//! shared immediates ([`IMMEDIATES_BYTES`](crate::gpu::pipeline::IMMEDIATES_BYTES)).

use crate::renderer::render_buffer::RenderBuffer;
use glam::{UVec2, Vec2};

/// The viewport size and the fragment origin as they appear in the shared immediates (16 bytes,
/// offset 0; see `Immediates` in `prelude.wgsl`).
#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct ViewportPush {
    pub(crate) size: Vec2,
    /// The surface pixel the target's pixel 0 is: zero, but on a damage canvas.
    pub(crate) origin: Vec2,
}

impl ViewportPush {
    pub(crate) const BYTES: usize = size_of::<Self>();
    /// Offset 0: the shared prelude puts `viewport_size` first.
    const OFFSET: u32 = 0;

    /// The viewport every pass of one frame pushes, so main and overlay passes agree.
    pub(crate) fn for_buffer(buffer: &RenderBuffer) -> Self {
        Self {
            size: buffer.display.physical.as_vec2(),
            origin: Vec2::ZERO,
        }
    }

    /// This viewport onto a target whose pixel 0 is the surface's `origin`.
    pub(crate) fn with_origin(self, origin: UVec2) -> Self {
        Self {
            origin: origin.as_vec2(),
            ..self
        }
    }

    fn encode(self) -> [u8; Self::BYTES] {
        bytemuck::cast(self)
    }

    /// Push this viewport into the active pipeline's immediate region; a pipeline must be bound.
    pub(crate) fn push_into(self, pass: &mut wgpu::RenderPass<'_>) {
        pass.set_immediates(Self::OFFSET, &self.encode());
    }
}

const _: () = assert!(
    ViewportPush::BYTES == 4 * size_of::<f32>(),
    "ViewportPush must match the shader's two vec2<f32> immediates",
);

#[cfg(test)]
mod tests {
    use crate::gpu::pipeline::quad_pipeline::QuadPipeline;
    use crate::gpu::pipeline::viewport_push::ViewportPush;
    use glam::{UVec2, Vec2};
    use std::mem::offset_of;

    #[test]
    fn viewport_immediate_is_four_native_endian_floats() {
        let encoded = ViewportPush {
            size: Vec2::new(1.5, -2.25),
            origin: Vec2::ZERO,
        }
        .with_origin(UVec2::new(64, 128))
        .encode();
        let mut expected = [0; ViewportPush::BYTES];
        for (at, v) in [1.5_f32, -2.25, 64.0, 128.0].into_iter().enumerate() {
            expected[at * 4..at * 4 + 4].copy_from_slice(&v.to_ne_bytes());
        }
        assert_eq!(encoded, expected);
    }

    /// The viewport push lies as the prelude's `Immediates` declares it, which every shader reads
    /// at `ViewportPush::OFFSET`.
    #[test]
    fn the_viewport_push_lies_as_the_shader_reads_it() {
        assert_eq!(ViewportPush::OFFSET, 0);
        QuadPipeline::SHADER.assert_struct_layout(
            "Immediates",
            ViewportPush::BYTES,
            &[
                ("viewport_size", offset_of!(ViewportPush, size)),
                ("frag_origin", offset_of!(ViewportPush, origin)),
            ],
        );
    }
}
