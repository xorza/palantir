//! A one-quad instance buffer that is written only when its quad changes.

use crate::gpu::dynamic_buffer::DynamicBuffer;
use crate::gpu::gpu_ctx::GpuCtx;
use crate::renderer::quad::Quad;

/// The instance buffer of a quad that changes rarely: the partial-repaint
/// pre-clear and the debug overlay's dim, both full-viewport quads that
/// change only with the viewport or the clear colour.
///
/// A 1-quad compare against the quad the buffer holds is what saves the
/// staging-belt write, unlike [`DynamicBuffer`]'s many-quad uploads,
/// where hashing the bytes costs more than sending them.
#[derive(Debug)]
pub(super) struct SingleQuadBuffer {
    buffer: DynamicBuffer<Quad>,
    /// What the buffer holds, `None` before the first upload.
    held: Option<Quad>,
}

impl SingleQuadBuffer {
    pub(super) fn new(device: &wgpu::Device, label: &'static str) -> Self {
        Self {
            buffer: DynamicBuffer::vertex(device, label, 1),
            held: None,
        }
    }

    /// Make the buffer hold `quad`, writing it only if it holds another.
    /// Answers whether it wrote.
    pub(super) fn upload(&mut self, ctx: &mut GpuCtx<'_>, quad: Quad) -> bool {
        if self
            .held
            .is_some_and(|held| bytemuck::bytes_of(&held) == bytemuck::bytes_of(&quad))
        {
            return false;
        }
        self.buffer.upload_instances(ctx, &[quad]);
        self.held = Some(quad);
        true
    }

    /// Whether any quad was uploaded yet.
    pub(super) const fn is_uploaded(&self) -> bool {
        self.held.is_some()
    }

    pub(super) const fn buffer(&self) -> &wgpu::Buffer {
        &self.buffer.buffer
    }
}

#[cfg(test)]
mod tests {
    use crate::gpu::gpu_ctx::GpuCtx;
    use crate::gpu::single_quad_buffer::SingleQuadBuffer;
    use crate::gpu::test_gpu::headless_test_gpu;
    use crate::primitives::color::RgbaF32;
    use crate::primitives::rect::Rect;
    use crate::renderer::quad::Quad;
    use wgpu::util::StagingBelt;

    /// A quad equal to the held one is not sent again; any change is,
    /// and a return to an earlier quad is a change too.
    #[test]
    fn writes_only_a_changed_quad() {
        let gpu = headless_test_gpu();
        let device = &gpu.device;
        let mut buffer = SingleQuadBuffer::new(device, "test.single_quad");
        let mut belt = StagingBelt::new(device.clone(), 1 << 12);
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        let mut ctx = GpuCtx::new(device, &gpu.queue, &mut belt, &mut encoder);
        let quad = |w: f32, alpha: f32| Quad {
            rect: Rect::new(0.0, 0.0, w, 100.0),
            fill: RgbaF32::new(0.0, 0.0, 0.0, alpha).into(),
            ..Default::default()
        };

        assert!(!buffer.is_uploaded());
        let writes = [
            (quad(200.0, 0.5), true),
            (quad(200.0, 0.5), false),
            (quad(300.0, 0.5), true),
            (quad(300.0, 1.0), true),
            (quad(200.0, 0.5), true),
            (quad(200.0, 0.5), false),
        ];
        for (at, (q, wrote)) in writes.into_iter().enumerate() {
            assert_eq!(buffer.upload(&mut ctx, q), wrote, "upload {at}");
        }
        assert!(buffer.is_uploaded());
    }
}
