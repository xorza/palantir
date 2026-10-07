//! `DynamicBuffer`: a `wgpu::Buffer` plus power-of-two growth. `upload_instances` grows the buffer
//! when the slice exceeds capacity and writes to offset 0: through the staging belt, or straight into
//! the new mapped buffer on a grow frame. No content-hash dedup: a belt memcpy is cheaper than FxHash
//! of the same bytes, so gating by hash is net-negative.
//!
//! Used by every pipeline (`quad`, `mesh`, `image`, `curve`) and the `text` backend's vbuf.

use crate::gpu::device::gpu_ctx::GpuCtx;
use std::marker::PhantomData;

#[derive(Debug)]
pub(crate) struct DynamicBuffer<T: bytemuck::Pod> {
    pub(crate) buffer: wgpu::Buffer,
    capacity: usize,
    usage: wgpu::BufferUsages,
    label: &'static str,
    item: PhantomData<T>,
}

impl<T: bytemuck::Pod> DynamicBuffer<T> {
    /// Bytes per item, checked when a buffer of `T` is instantiated: a zero-sized row holds nothing,
    /// and the belt copy and mapped write both need whole rows in multiples of
    /// [`wgpu::COPY_BUFFER_ALIGNMENT`].
    const ITEM_BYTES: usize = {
        let size = size_of::<T>();
        assert!(size != 0, "DynamicBuffer does not support zero-sized rows");
        assert!(
            size.is_multiple_of(wgpu::COPY_BUFFER_ALIGNMENT as usize),
            "a DynamicBuffer row must be a multiple of COPY_BUFFER_ALIGNMENT bytes",
        );
        size
    };

    /// A vertex/instance buffer for items of type `T`: `VERTEX | COPY_DST`, the common case for the
    /// four pipelines and the debug overlay.
    pub(crate) fn vertex(
        device: &wgpu::Device,
        label: &'static str,
        initial_capacity: usize,
    ) -> Self {
        Self::new(
            device,
            label,
            wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            initial_capacity,
        )
    }

    /// An index buffer for items of type `T`: `INDEX | COPY_DST`.
    pub(crate) fn index(
        device: &wgpu::Device,
        label: &'static str,
        initial_capacity: usize,
    ) -> Self {
        Self::new(
            device,
            label,
            wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
            initial_capacity,
        )
    }

    fn new(
        device: &wgpu::Device,
        label: &'static str,
        usage: wgpu::BufferUsages,
        initial_capacity: usize,
    ) -> Self {
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(label),
            size: (initial_capacity * Self::ITEM_BYTES) as u64,
            usage,
            mapped_at_creation: false,
        });
        Self {
            buffer,
            capacity: initial_capacity,
            usage,
            label,
            item: PhantomData,
        }
    }

    /// Grows if needed and writes `items` to offset 0. On a grow frame the new buffer is created
    /// `mapped_at_creation: true` and the bytes are memcpy'd straight into the mapped range, with no
    /// belt staging copy or `copy_buffer_to_buffer`.
    fn upload(&mut self, ctx: &mut GpuCtx<'_>, items: &[T]) {
        let bytes = bytemuck::cast_slice(items);
        if self.grow_mapped(ctx.device, items.len()) {
            self.buffer
                .slice(..bytes.len() as u64)
                .get_mapped_range_mut()
                .expect("map mapped-at-creation range")
                .copy_from_slice(bytes);
            self.buffer.unmap();
            return;
        }
        ctx.write(&self.buffer, 0, bytes);
    }

    /// Uploads a slice of `Pod` instances to offset 0 (no-op when empty). The empty guard, `cast_slice`
    /// and count are identical across every instanced pipeline, so they live here.
    pub(crate) fn upload_instances(&mut self, ctx: &mut GpuCtx<'_>, items: &[T]) {
        if items.is_empty() {
            return;
        }
        self.upload(ctx, items);
    }

    /// Grows to fit `needed_len` items with the new buffer `mapped_at_creation: true`. Returns `true`
    /// when recreated (the caller writes into the mapped range, then calls `unmap`), `false` when
    /// capacity already fit (the caller takes the belt path).
    fn grow_mapped(&mut self, device: &wgpu::Device, needed_len: usize) -> bool {
        if needed_len <= self.capacity {
            return false;
        }
        self.capacity = grown_capacity(needed_len);
        self.buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(self.label),
            size: (self.capacity * Self::ITEM_BYTES) as u64,
            usage: self.usage,
            mapped_at_creation: true,
        });
        true
    }
}

const fn grown_capacity(needed_len: usize) -> usize {
    needed_len.next_power_of_two()
}

#[cfg(test)]
mod tests {
    use super::grown_capacity;

    #[test]
    fn growth_rounds_to_the_next_power_of_two() {
        assert_eq!(grown_capacity(1), 1);
        assert_eq!(grown_capacity(2), 2);
        assert_eq!(grown_capacity(3), 4);
        assert_eq!(grown_capacity(257), 512);
    }
}
