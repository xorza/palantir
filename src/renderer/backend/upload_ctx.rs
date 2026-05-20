//! Per-frame upload context: the three handles every dynamic upload
//! needs, bundled so callers thread one `&mut UploadCtx` instead of
//! `(&device, &mut belt, &mut encoder)` triples.
//!
//! Lifetimes are tied together so the renderer can `let mut ctx =
//! UploadCtx::new(...)` once after creating the main encoder and pass
//! `&mut ctx` to every uploader in the frame. Dropping the ctx
//! releases all three borrows so render passes can resume using the
//! encoder.

pub struct UploadCtx<'a> {
    pub device: &'a wgpu::Device,
    pub belt: &'a mut wgpu::util::StagingBelt,
    pub encoder: &'a mut wgpu::CommandEncoder,
}

impl<'a> UploadCtx<'a> {
    pub fn new(
        device: &'a wgpu::Device,
        belt: &'a mut wgpu::util::StagingBelt,
        encoder: &'a mut wgpu::CommandEncoder,
    ) -> Self {
        Self {
            device,
            belt,
            encoder,
        }
    }

    /// Schedule a belt-backed `copy_buffer_to_buffer` from staging to
    /// `dst@offset`. Empty `bytes` is a no-op (wgpu's
    /// `BufferSize::new` rejects zero). `offset` and `bytes.len()`
    /// must both be multiples of `COPY_BUFFER_ALIGNMENT` (4).
    pub fn write(&mut self, dst: &wgpu::Buffer, offset: u64, bytes: &[u8]) {
        let Some(size) = wgpu::BufferSize::new(bytes.len() as u64) else {
            return;
        };
        let mut view = self.belt.write_buffer(self.encoder, dst, offset, size);
        view.copy_from_slice(bytes);
    }
}
