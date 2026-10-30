//! Per-frame GPU handles bundled so callers thread one `&mut GpuCtx`, built right after the main
//! encoder; dropping it releases the borrows.

use crate::gpu::device::gpu::Gpu;
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
        gpu: &'a Gpu,
        belt: &'a mut util::StagingBelt,
        encoder: &'a mut wgpu::CommandEncoder,
    ) -> Self {
        Self {
            device: &gpu.device,
            queue: &gpu.queue,
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

    /// `bytes` of mapped belt staging at an offset aligned to `alignment`, for a copy the caller
    /// records itself, as into a texture, which [`Self::write`] cannot reach; `bytes` must be a
    /// multiple of `COPY_BUFFER_ALIGNMENT` (4) and `alignment` a power of two.
    ///
    /// `None` for no bytes, and on a lost device, whose belt allocations do not map: the loss can
    /// come inside the frame, before any call reports it, and the copy would be dropped as every
    /// call on a lost device is.
    pub(crate) fn stage(&mut self, bytes: u64, alignment: u64) -> Option<Staged<'_>> {
        let size = wgpu::BufferSize::new(bytes)?;
        let alignment =
            wgpu::BufferSize::new(alignment).expect("a staging alignment is a power of two");
        let slice = self.belt.allocate(size, alignment);
        let view = slice.get_mapped_range_mut().ok()?;
        Some(Staged {
            view,
            buffer: slice.buffer(),
            offset: slice.offset(),
            encoder: self.encoder,
        })
    }

    /// [`Self::write`] without the source slice: the mapped staging bytes, for a caller composing in place (one memcpy, not two). Unwritten bytes keep the belt chunk's old contents and must never be read. `None` as [`Self::stage`] gives it.
    ///
    /// Not `StagingBelt::write_buffer`, which panics where an allocation does not map.
    pub(crate) fn write_view(
        &mut self,
        dst: &wgpu::Buffer,
        offset: u64,
        bytes: u64,
    ) -> Option<wgpu::BufferViewMut> {
        debug_assert!(
            offset.is_multiple_of(wgpu::COPY_BUFFER_ALIGNMENT),
            "a buffer copy starts on a multiple of `COPY_BUFFER_ALIGNMENT`"
        );
        let Staged {
            view,
            buffer,
            offset: from,
            encoder,
        } = self.stage(bytes, wgpu::COPY_BUFFER_ALIGNMENT)?;
        encoder.copy_buffer_to_buffer(buffer, from, dst, offset, bytes);
        Some(view)
    }
}

/// Belt staging [`GpuCtx::stage`] handed out: the mapped bytes, where they sit, and the encoder
/// the copies out of them record on, submitted with the belt's own.
#[derive(Debug)]
pub(crate) struct Staged<'a> {
    pub(crate) view: wgpu::BufferViewMut,
    pub(crate) buffer: &'a wgpu::Buffer,
    pub(crate) offset: u64,
    pub(crate) encoder: &'a mut wgpu::CommandEncoder,
}
