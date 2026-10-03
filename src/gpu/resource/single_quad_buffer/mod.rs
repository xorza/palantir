//! A one-quad instance buffer that is written only when its quad changes.

use crate::gpu::device::gpu_ctx::GpuCtx;
use crate::gpu::resource::dynamic_buffer::DynamicBuffer;
use crate::renderer::quad::Quad;

/// The instance buffer of a quad that changes rarely: the partial-repaint
/// pre-clear and the debug overlay's dim, both full-viewport quads that
/// change only with the viewport or the clear colour.
///
/// A 1-quad compare against the quad the buffer holds is what saves the
/// staging-belt write, unlike [`DynamicBuffer`]'s many-quad uploads,
/// where hashing the bytes costs more than sending them.
#[derive(Debug)]
pub(crate) struct SingleQuadBuffer {
    buffer: DynamicBuffer<Quad>,
    /// What the buffer holds, `None` before the first upload.
    held: Option<Quad>,
}

impl SingleQuadBuffer {
    pub(crate) fn new(device: &wgpu::Device, label: &'static str) -> Self {
        Self {
            buffer: DynamicBuffer::vertex(device, label, 1),
            held: None,
        }
    }

    /// Make the buffer hold `quad`, writing it only if it holds another.
    /// Answers whether it wrote.
    pub(crate) fn upload(&mut self, ctx: &mut GpuCtx<'_>, quad: Quad) -> bool {
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
    pub(crate) const fn is_uploaded(&self) -> bool {
        self.held.is_some()
    }

    pub(crate) const fn buffer(&self) -> &wgpu::Buffer {
        &self.buffer.buffer
    }
}

#[cfg(test)]
mod tests;
