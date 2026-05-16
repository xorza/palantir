//! Small shared helpers for the wgpu pipelines (quad / mesh / image).
//! Extracted to kill the three-way duplicated regrow-instance-buffer
//! pattern; future work (`docs/review-renderer-backend.md` A1) can
//! grow this into a full `build_pipeline` / `build_stencil_variant`
//! recipe.

/// Grow `buffer` to fit `needed_len` items of `item_size` bytes,
/// rounding up to the next power of two (floored at `min_capacity`).
/// `capacity` tracks the slot count, not bytes. No-op when `needed_len
/// <= *capacity`. Single source of truth for the wgpu vertex/index/
/// instance buffer regrow pattern — see `quad_pipeline.rs`,
/// `mesh_pipeline.rs`, `image_pipeline.rs`.
#[allow(clippy::too_many_arguments)]
pub(super) fn grow_instance_buffer(
    device: &wgpu::Device,
    buffer: &mut wgpu::Buffer,
    capacity: &mut usize,
    needed_len: usize,
    item_size: usize,
    usage: wgpu::BufferUsages,
    label: &'static str,
    min_capacity: usize,
) {
    if needed_len <= *capacity {
        return;
    }
    *capacity = needed_len.next_power_of_two().max(min_capacity);
    *buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(label),
        size: (*capacity * item_size) as u64,
        usage,
        mapped_at_creation: false,
    });
}
