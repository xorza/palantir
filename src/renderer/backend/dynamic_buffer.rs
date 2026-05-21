//! `DynamicBuffer` — a `wgpu::Buffer` plus power-of-two growth.
//!
//! `buf.upload(ctx, bytes, count)` grows the underlying buffer when
//! `count` exceeds capacity, then schedules a belt-backed
//! `copy_buffer_to_buffer` to offset 0. No content-hash deduplication:
//! staging-belt memcpy is cheaper than FxHash of the same bytes, so
//! gating by hash is always net-negative.
//!
//! Used by every pipeline (`quad`, `mesh`, `image`, `curve`) plus
//! `text_backend`'s vbuf.

use super::gpu_ctx::GpuCtx;

pub(crate) struct DynamicBuffer {
    pub(crate) buffer: wgpu::Buffer,
    capacity: usize,
    item_size: usize,
    usage: wgpu::BufferUsages,
    label: &'static str,
    /// Floor for the power-of-two regrow — keeps tiny first-frame
    /// uploads from creating a 1-slot buffer that immediately doubles.
    min_capacity: usize,
}

impl DynamicBuffer {
    /// Construct a vertex/instance buffer for items of type `T`.
    /// `VERTEX | COPY_DST` usage (the common case for the four
    /// pipelines and the debug overlay). Item size comes from
    /// `size_of::<T>()` so call sites don't repeat it.
    pub(crate) fn vertex<T>(
        device: &wgpu::Device,
        label: &'static str,
        initial_capacity: usize,
        min_capacity: usize,
    ) -> Self {
        Self::new::<T>(
            device,
            label,
            wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            initial_capacity,
            min_capacity,
        )
    }

    /// Construct an index buffer for items of type `T` (typically `u16`).
    /// `INDEX | COPY_DST` usage.
    pub(crate) fn index<T>(
        device: &wgpu::Device,
        label: &'static str,
        initial_capacity: usize,
        min_capacity: usize,
    ) -> Self {
        Self::new::<T>(
            device,
            label,
            wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
            initial_capacity,
            min_capacity,
        )
    }

    fn new<T>(
        device: &wgpu::Device,
        label: &'static str,
        usage: wgpu::BufferUsages,
        initial_capacity: usize,
        min_capacity: usize,
    ) -> Self {
        let item_size = std::mem::size_of::<T>();
        let capacity = initial_capacity.max(min_capacity);
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(label),
            size: (capacity * item_size) as u64,
            usage,
            mapped_at_creation: false,
        });
        Self {
            buffer,
            capacity,
            item_size,
            usage,
            label,
            min_capacity,
        }
    }

    /// Common case: grow if needed, schedule a belt write to offset 0.
    /// `bytes.len()` must equal `item_count * self.item_size`.
    pub(crate) fn upload(&mut self, ctx: &mut GpuCtx<'_>, bytes: &[u8], item_count: usize) {
        self.upload_with(ctx, item_count, |dst, ctx| {
            ctx.write(dst, 0, bytes);
        });
    }

    /// Generic upload path for callers that need more than one belt
    /// write per logical upload (e.g. the mesh index buffer's
    /// odd-length padded path schedules two copies to honor wgpu's
    /// 4-byte copy alignment). The `write` closure receives
    /// `(&dst_buffer, &mut ctx)`.
    pub(crate) fn upload_with<F>(&mut self, ctx: &mut GpuCtx<'_>, item_count: usize, write: F)
    where
        F: FnOnce(&wgpu::Buffer, &mut GpuCtx<'_>),
    {
        self.grow(ctx.device, item_count);
        write(&self.buffer, ctx);
    }

    /// Grow to fit `needed_len` items, rounding up to the next power
    /// of two (floored at `min_capacity`).
    fn grow(&mut self, device: &wgpu::Device, needed_len: usize) {
        if needed_len <= self.capacity {
            return;
        }
        self.capacity = needed_len.next_power_of_two().max(self.min_capacity);
        self.buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(self.label),
            size: (self.capacity * self.item_size) as u64,
            usage: self.usage,
            mapped_at_creation: false,
        });
    }
}
