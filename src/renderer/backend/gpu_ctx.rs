//! Per-frame GPU-handle bundle: the four references every uploader
//! and texture-write path needs, bundled so callers thread one
//! `&mut GpuCtx` instead of `(&device, &queue, &mut belt, &mut encoder)`
//! quadruples.
//!
//! - `device` — lazy buffer / texture regrow.
//! - `queue` — `write_texture` for the rare image-registry + gradient
//!   atlas paths (staging-belt covers `write_buffer` only).
//! - `belt` — sub-allocates mapped staging memory for buffer uploads.
//! - `encoder` — records `copy_buffer_to_buffer` from staging to dst,
//!   plus the user's render passes.
//!
//! Lifetimes are tied together so the renderer constructs one ctx
//! right after creating the main encoder and passes `&mut ctx` to
//! every uploader. Dropping the ctx releases all four borrows so
//! render passes can resume using the encoder afterward.

#[derive(Debug)]
pub(super) struct GpuCtx<'a> {
    pub(super) device: &'a wgpu::Device,
    pub(super) queue: &'a wgpu::Queue,
    belt: &'a mut wgpu::util::StagingBelt,
    pub(super) encoder: &'a mut wgpu::CommandEncoder,
}

impl<'a> GpuCtx<'a> {
    pub(super) fn new(
        device: &'a wgpu::Device,
        queue: &'a wgpu::Queue,
        belt: &'a mut wgpu::util::StagingBelt,
        encoder: &'a mut wgpu::CommandEncoder,
    ) -> Self {
        Self {
            device,
            queue,
            belt,
            encoder,
        }
    }

    /// Schedule a belt-backed `copy_buffer_to_buffer` from staging to
    /// `dst@offset`. Empty `bytes` is a no-op (wgpu's
    /// `BufferSize::new` rejects zero). `offset` and `bytes.len()`
    /// must both be multiples of `COPY_BUFFER_ALIGNMENT` (4).
    pub(super) fn write(&mut self, dst: &wgpu::Buffer, offset: u64, bytes: &[u8]) {
        let Some(mut view) = self.write_view(dst, offset, bytes.len() as u64) else {
            return;
        };
        view.copy_from_slice(bytes);
    }

    /// [`Self::write`] without the source slice: the mapped staging
    /// bytes themselves, for a caller that composes them in place.
    ///
    /// What that buys is one memcpy instead of two. A caller holding the
    /// finished bytes already should use [`Self::write`] — this is for
    /// one that would otherwise build a full-size copy just to hand it
    /// over, which is the whole upload's worth of bytes staged twice.
    /// Unwritten bytes of the view keep whatever the belt's chunk last
    /// held, so a caller that leaves gaps owes it that they are never
    /// read.
    pub(super) fn write_view(
        &mut self,
        dst: &wgpu::Buffer,
        offset: u64,
        bytes: u64,
    ) -> Option<wgpu::BufferViewMut> {
        let size = wgpu::BufferSize::new(bytes)?;
        Some(self.belt.write_buffer(self.encoder, dst, offset, size))
    }
}
