//! `DynamicBuffer` — a `wgpu::Buffer` plus power-of-two growth.
//!
//! `buf.upload(ctx, bytes, count)` grows the underlying buffer when
//! `count` exceeds capacity, then schedules a belt-backed
//! `copy_buffer_to_buffer` to offset 0. No content-hash deduplication:
//! staging-belt memcpy is cheaper than FxHash of the same bytes, so
//! gating by hash is always net-negative.
//!
//! Used by every pipeline (`quad`, `mesh`, `image`, `curve`) plus
//! the `text` backend's vbuf.

use crate::renderer::backend::gpu_ctx::GpuCtx;
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
    /// Construct a vertex/instance buffer for items of type `T`.
    /// `VERTEX | COPY_DST` usage (the common case for the four
    /// pipelines and the debug overlay). Item size comes from
    /// `size_of::<T>()` so call sites don't repeat it.
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

    /// Construct an index buffer for items of type `T` (typically `u16`).
    /// `INDEX | COPY_DST` usage.
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
        let item_size = std::mem::size_of::<T>();
        assert!(
            item_size != 0,
            "DynamicBuffer does not support zero-sized rows"
        );
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(label),
            size: (initial_capacity * item_size) as u64,
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

    /// Grow if needed, write `bytes` to offset 0. On a grow frame the
    /// new buffer is created `mapped_at_creation: true` and the bytes
    /// are memcpy'd straight into the mapped range — no belt staging
    /// copy, no `copy_buffer_to_buffer` recorded. `bytes.len()` must
    /// equal `item_count * self.item_size`.
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

    /// Upload a slice of `Pod` instances to offset 0 (no-op when empty).
    /// The empty-guard + `cast_slice` + count are identical across every
    /// instanced pipeline, so they live here rather than re-spelled per
    /// pipeline.
    pub(crate) fn upload_instances(&mut self, ctx: &mut GpuCtx<'_>, items: &[T]) {
        if items.is_empty() {
            return;
        }
        self.upload(ctx, items);
    }

    /// Grow to fit `needed_len` items with the new buffer
    /// `mapped_at_creation: true`. Returns `true` when the buffer was
    /// recreated (caller must write into the mapped range then call
    /// `unmap`); `false` when the existing buffer's capacity already
    /// fit (caller takes the belt path).
    fn grow_mapped(&mut self, device: &wgpu::Device, needed_len: usize) -> bool {
        if needed_len <= self.capacity {
            return false;
        }
        self.capacity = grown_capacity(needed_len);
        self.buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(self.label),
            size: (self.capacity * std::mem::size_of::<T>()) as u64,
            usage: self.usage,
            mapped_at_creation: true,
        });
        true
    }
}

fn grown_capacity(needed_len: usize) -> usize {
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
