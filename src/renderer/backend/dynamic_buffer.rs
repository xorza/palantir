//! `DynamicBuffer` — a `wgpu::Buffer` plus growth metadata plus a
//! per-frame content-hash gate.
//!
//! Each per-frame upload is `buf.upload(ctx, bytes, count)`:
//! - hashes `bytes`,
//! - grows the underlying buffer when `count` exceeds capacity,
//! - skips the belt write entirely when the hash matches last frame's
//!   (and the buffer wasn't just reallocated).
//!
//! On Metal each `queue.write_buffer` allocates a fresh
//! `MTLBlitCommandEncoder` (~26% self-time in `begin_encoding` per the
//! frame-bench profile). Routing through the
//! [`GpuCtx`](super::gpu_ctx::GpuCtx)'s staging belt collapses
//! N writes into N `copy_buffer_to_buffer` commands on one encoder.
//! The hash gate stays valuable on top — a content match skips the
//! belt allocation + copy command + memcpy.
//!
//! Used by every pipeline (`quad`, `mesh`, `image`, `curve`) plus
//! `text_backend`'s vbuf.

use super::gpu_ctx::GpuCtx;

pub(crate) struct DynamicBuffer {
    buffer: wgpu::Buffer,
    capacity: usize,
    last_hash: Option<u64>,
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
            last_hash: None,
            item_size,
            usage,
            label,
            min_capacity,
        }
    }

    /// Handle for binding into render passes / index slots.
    pub(crate) fn buffer(&self) -> &wgpu::Buffer {
        &self.buffer
    }

    /// Common case: hash `bytes`, grow if needed, schedule a belt
    /// write to offset 0. Returns `true` when a belt write actually
    /// fired. `bytes.len()` must equal `item_count * self.item_size`.
    pub(crate) fn upload(&mut self, ctx: &mut GpuCtx<'_>, bytes: &[u8], item_count: usize) -> bool {
        self.upload_with(ctx, item_count, hash_bytes(bytes), |dst, ctx| {
            ctx.write(dst, 0, bytes);
        })
    }

    /// Generic upload path for callers that need more than one belt
    /// write per logical upload (e.g. the mesh index buffer's
    /// odd-length padded path schedules two copies to honor wgpu's
    /// 4-byte copy alignment). `content_hash` must reflect the full
    /// logical payload so the gate is correct across calling shapes.
    /// The `write` closure receives `(&dst_buffer, &mut ctx)` and is
    /// invoked only when the gate decides a write is needed.
    pub(crate) fn upload_with<F>(
        &mut self,
        ctx: &mut GpuCtx<'_>,
        item_count: usize,
        content_hash: u64,
        write: F,
    ) -> bool
    where
        F: FnOnce(&wgpu::Buffer, &mut GpuCtx<'_>),
    {
        let grew = self.grow(ctx.device, item_count);
        if !grew && self.last_hash == Some(content_hash) {
            return false;
        }
        write(&self.buffer, ctx);
        self.last_hash = Some(content_hash);
        true
    }

    /// Grow to fit `needed_len` items, rounding up to the next power
    /// of two (floored at `min_capacity`). Returns `true` when the
    /// buffer was reallocated — the gate uses this to force the next
    /// write through (fresh buffer = undefined contents, hash match
    /// would be a stale skip).
    fn grow(&mut self, device: &wgpu::Device, needed_len: usize) -> bool {
        if needed_len <= self.capacity {
            return false;
        }
        self.capacity = needed_len.next_power_of_two().max(self.min_capacity);
        self.buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(self.label),
            size: (self.capacity * self.item_size) as u64,
            usage: self.usage,
            mapped_at_creation: false,
        });
        true
    }
}

fn hash_bytes(bytes: &[u8]) -> u64 {
    use std::hash::Hasher as _;
    let mut h = crate::common::hash::Hasher::new();
    h.write(bytes);
    h.finish()
}
