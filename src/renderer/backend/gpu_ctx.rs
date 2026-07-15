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

use crate::renderer::backend::queue::Queue;

#[derive(Debug)]
pub(crate) struct GpuCtx<'a> {
    pub(crate) device: &'a wgpu::Device,
    pub(crate) queue: &'a Queue,
    pub(crate) belt: &'a mut wgpu::util::StagingBelt,
    pub(crate) encoder: &'a mut wgpu::CommandEncoder,
}

impl<'a> GpuCtx<'a> {
    pub(crate) fn new(
        device: &'a wgpu::Device,
        queue: &'a Queue,
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
    pub(crate) fn write(&mut self, dst: &wgpu::Buffer, offset: u64, bytes: &[u8]) {
        let Some(size) = wgpu::BufferSize::new(bytes.len() as u64) else {
            return;
        };
        let mut view = self.belt.write_buffer(self.encoder, dst, offset, size);
        view.copy_from_slice(bytes);
    }
}

#[cfg(feature = "internals")]
pub(crate) mod test_support {
    use crate::renderer::backend::gpu_ctx::GpuCtx as InnerGpuCtx;
    use crate::renderer::backend::queue::test_support::Queue;

    #[derive(Debug)]
    pub struct GpuCtx<'a>(pub(crate) InnerGpuCtx<'a>);

    impl<'a> GpuCtx<'a> {
        pub fn new(
            device: &'a wgpu::Device,
            queue: &'a Queue,
            belt: &'a mut wgpu::util::StagingBelt,
            encoder: &'a mut wgpu::CommandEncoder,
        ) -> Self {
            Self(InnerGpuCtx::new(device, &queue.0, belt, encoder))
        }
    }
}
