//! The [`ViewportPush`] carrier for `imm.viewport_size`, offset 0 of the shared immediates
//! ([`IMMEDIATES_BYTES`](crate::gpu::pipeline::IMMEDIATES_BYTES)).

use crate::renderer::render_buffer::RenderBuffer;
use glam::Vec2;

/// Viewport size as it appears in the shared immediate (8 bytes, offset 0; see `Immediates` in `prelude.wgsl`).
#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct ViewportPush {
    pub(crate) size: Vec2,
}

impl ViewportPush {
    pub(crate) const BYTES: usize = size_of::<Self>();
    /// Offset 0: the shared prelude puts `viewport_size` first.
    const OFFSET: u32 = 0;

    /// The viewport every pass of one frame pushes, so main and overlay passes agree.
    pub(crate) fn for_buffer(buffer: &RenderBuffer) -> Self {
        Self {
            size: buffer.display.physical.as_vec2(),
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
    ViewportPush::BYTES == 2 * size_of::<f32>(),
    "ViewportPush must match the shader's vec2<f32> viewport layout",
);

#[cfg(test)]
mod tests {
    use crate::gpu::pipeline::quad_pipeline::QuadPipeline;
    use crate::gpu::pipeline::viewport_push::ViewportPush;
    use glam::Vec2;
    use std::mem::offset_of;

    #[test]
    fn viewport_immediate_is_two_native_endian_floats() {
        let encoded = ViewportPush {
            size: Vec2::new(1.5, -2.25),
        }
        .encode();
        let mut expected = [0; ViewportPush::BYTES];
        expected[..4].copy_from_slice(&1.5_f32.to_ne_bytes());
        expected[4..].copy_from_slice(&(-2.25_f32).to_ne_bytes());
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
            &[("viewport_size", offset_of!(ViewportPush, size))],
        );
    }
}
