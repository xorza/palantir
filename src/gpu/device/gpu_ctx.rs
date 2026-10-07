//! Per-frame GPU handles bundled so callers thread one `&mut GpuCtx`.
//!
//! - `device`: lazy buffer / texture regrow.
//! - `queue`: `write_texture` for image-registry and gradient atlas paths (the belt covers `write_buffer` only).
//! - `belt`: sub-allocates mapped staging memory.
//! - `encoder`: records staging-to-destination copies and the user's render passes.
//!
//! Built right after the main encoder; dropping it releases the borrows.

use wgpu::util;
#[derive(Debug)]
pub(crate) struct GpuCtx<'a> {
    pub(crate) device: &'a wgpu::Device,
    pub(crate) queue: &'a wgpu::Queue,
    belt: &'a mut util::StagingBelt,
    pub(crate) encoder: &'a mut wgpu::CommandEncoder,
}

impl<'a> GpuCtx<'a> {
    pub(crate) const fn new(
        device: &'a wgpu::Device,
        queue: &'a wgpu::Queue,
        belt: &'a mut util::StagingBelt,
        encoder: &'a mut wgpu::CommandEncoder,
    ) -> Self {
        Self {
            device,
            queue,
            belt,
            encoder,
        }
    }

    /// Schedule a belt-backed copy from staging to `dst@offset`. Empty `bytes` is a no-op; `offset` and length must be multiples of `COPY_BUFFER_ALIGNMENT` (4).
    pub(crate) fn write(&mut self, dst: &wgpu::Buffer, offset: u64, bytes: &[u8]) {
        let Some(mut view) = self.write_view(dst, offset, bytes.len() as u64) else {
            return;
        };
        view.copy_from_slice(bytes);
    }

    /// [`Self::write`] without the source slice: the mapped staging bytes, for a caller composing in place (one memcpy, not two). Unwritten bytes keep the belt chunk's old contents and must never be read.
    pub(crate) fn write_view(
        &mut self,
        dst: &wgpu::Buffer,
        offset: u64,
        bytes: u64,
    ) -> Option<wgpu::BufferViewMut> {
        let size = wgpu::BufferSize::new(bytes)?;
        Some(self.belt.write_buffer(self.encoder, dst, offset, size))
    }
}
