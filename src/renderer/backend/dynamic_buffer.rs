//! A vertex/index/instance buffer that grows on demand and skips
//! redundant `queue.write_buffer` calls when its contents haven't
//! changed since last frame.
//!
//! On Metal each `queue.write_buffer` allocates a fresh
//! `MTLBlitCommandEncoder` (~26% self-time in `begin_encoding` per the
//! frame-bench profile), so a 64-bit FxHash + skip is cheaper than the
//! write whenever the content matches. Steady-state Partial frames see
//! ~50% fewer per-frame writes; resizing frames see ~10%.
//!
//! Owns `(buffer, capacity, last_upload_hash, creation params)` as a
//! single unit. Each per-frame upload is a one-liner — `buf.upload(...)`
//! — that hashes the input, grows the underlying buffer when item
//! count exceeds capacity, and only invokes `queue.write_buffer` when
//! the content hash differs (or the buffer was just reallocated and
//! holds undefined contents). Used by all four pipeline modules
//! (`quad_pipeline`, `mesh_pipeline`, `image_pipeline`,
//! `curve_pipeline`).

use super::Queue;

pub(super) struct DynamicBuffer {
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
    pub(super) fn new(
        device: &wgpu::Device,
        label: &'static str,
        usage: wgpu::BufferUsages,
        item_size: usize,
        initial_capacity: usize,
        min_capacity: usize,
    ) -> Self {
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
    pub(super) fn buffer(&self) -> &wgpu::Buffer {
        &self.buffer
    }

    /// Common case: hash `bytes`, grow if needed, write at offset 0.
    /// Returns `true` when a `queue.write_buffer` actually fired.
    /// `bytes.len()` must equal `item_count * self.item_size`.
    pub(super) fn upload(
        &mut self,
        device: &wgpu::Device,
        queue: &Queue,
        bytes: &[u8],
        item_count: usize,
    ) -> bool {
        self.upload_with(device, item_count, hash_bytes(bytes), |buf| {
            queue.write_buffer(buf, 0, bytes);
        })
    }

    /// Generic upload path for callers that need more than one
    /// `write_buffer` per logical upload (e.g. the mesh index buffer's
    /// odd-length padded path issues two writes to honor wgpu's 4-byte
    /// copy alignment). `content_hash` must reflect the full logical
    /// payload so the gate is correct across calling shapes. The
    /// `write` closure is invoked only when the gate decides a write
    /// is needed.
    pub(super) fn upload_with<F: FnOnce(&wgpu::Buffer)>(
        &mut self,
        device: &wgpu::Device,
        item_count: usize,
        content_hash: u64,
        write: F,
    ) -> bool {
        let grew = self.grow(device, item_count);
        if !grew && self.last_hash == Some(content_hash) {
            return false;
        }
        write(&self.buffer);
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
