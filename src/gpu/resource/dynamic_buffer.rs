//! `DynamicBuffer`: a `wgpu::Buffer` plus power-of-two growth and decay. `upload_instances` grows
//! the buffer when the slice exceeds capacity and writes to offset 0: through the staging belt, or
//! into the new buffer as it is created on a grow frame. At the end of each expiry window it
//! steps down to the window's peak, as AGENTS.md's "Scratch decays" asks of per-frame scratch:
//! one frame of a million glyph quads leaves no 32 MiB buffer for the window's life. No
//! content-hash dedup: a belt memcpy is cheaper than FxHash of the same bytes, so gating by hash
//! is net-negative.
//!
//! Used by every pipeline (`quad`, `mesh`, `image`, `curve`), the `text` backend's vbuf, the blur's
//! params and the backdrop slot table.

use crate::gpu::device::gpu::Gpu;
use crate::gpu::device::gpu_ctx::GpuCtx;
use crate::gpu::resource::expiry_window::ExpiryWindow;
use std::marker::PhantomData;

#[derive(Debug)]
pub(crate) struct DynamicBuffer<T: bytemuck::Pod> {
    pub(crate) buffer: wgpu::Buffer,
    capacity: usize,
    /// The capacity it was made at, which it never steps under.
    initial_capacity: usize,
    /// The most items an upload of the current expiry window held.
    peak: usize,
    window: ExpiryWindow,
    usage: wgpu::BufferUsages,
    label: &'static str,
    item: PhantomData<T>,
}

/// What [`DynamicBuffer::new`] builds: the buffer's name, what it binds as, and
/// the items it holds before its first grow.
#[derive(Clone, Copy, Debug)]
struct BufferSpec {
    label: &'static str,
    usage: wgpu::BufferUsages,
    initial_capacity: usize,
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
    pub(crate) fn vertex(gpu: &Gpu, label: &'static str, initial_capacity: usize) -> Self {
        Self::new(
            gpu,
            BufferSpec {
                label,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                initial_capacity,
            },
        )
    }

    /// A read-only storage buffer for items of type `T`: `STORAGE | COPY_DST`.
    pub(crate) fn storage(gpu: &Gpu, label: &'static str, initial_capacity: usize) -> Self {
        Self::new(
            gpu,
            BufferSpec {
                label,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                initial_capacity,
            },
        )
    }

    /// The items the buffer holds before it grows; a grow replaces the buffer, so a bind group
    /// over it is built again when this changes.
    pub(crate) const fn capacity(&self) -> usize {
        self.capacity
    }

    /// A uniform buffer of items of type `T`, each bound at a dynamic offset: `UNIFORM | COPY_DST`.
    /// A `T` is padded to the device's uniform offset alignment.
    pub(crate) fn uniform(gpu: &Gpu, label: &'static str, initial_capacity: usize) -> Self {
        Self::new(
            gpu,
            BufferSpec {
                label,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                initial_capacity,
            },
        )
    }

    /// An index buffer for items of type `T`: `INDEX | COPY_DST`.
    pub(crate) fn idx(gpu: &Gpu, label: &'static str, initial_capacity: usize) -> Self {
        Self::new(
            gpu,
            BufferSpec {
                label,
                usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
                initial_capacity,
            },
        )
    }

    fn new(gpu: &Gpu, spec: BufferSpec) -> Self {
        let BufferSpec {
            label,
            usage,
            initial_capacity,
        } = spec;
        let buffer = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(label),
            size: (initial_capacity * Self::ITEM_BYTES) as u64,
            usage,
            mapped_at_creation: false,
        });
        Self {
            buffer,
            capacity: initial_capacity,
            initial_capacity,
            peak: 0,
            window: ExpiryWindow::default(),
            usage,
            label,
            item: PhantomData,
        }
    }

    /// Writes `items` to offset 0 of a buffer of `wanted` items: a new one where `wanted` moved,
    /// created holding the bytes, with no belt staging copy or `copy_buffer_to_buffer`
    /// ([`Gpu::create_filled_buffer`](crate::gpu::device::gpu::Gpu::create_filled_buffer)).
    fn upload(&mut self, ctx: &mut GpuCtx<'_>, items: &[T], wanted: usize) {
        let bytes = bytemuck::cast_slice(items);
        if wanted != self.capacity {
            self.capacity = wanted;
            self.buffer = ctx.gpu.create_filled_buffer(
                &wgpu::BufferDescriptor {
                    label: Some(self.label),
                    size: (self.capacity * Self::ITEM_BYTES) as u64,
                    usage: self.usage,
                    mapped_at_creation: true,
                },
                bytes,
            );
            return;
        }
        ctx.write(&self.buffer, 0, bytes);
    }

    /// Uploads a slice of `Pod` instances to offset 0. An empty slice writes nothing, and still
    /// counts toward the window's peak, so a buffer a feature stopped using steps down. The
    /// guard, `cast_slice` and count are identical across every instanced pipeline, so they live
    /// here.
    pub(crate) fn upload_instances(&mut self, ctx: &mut GpuCtx<'_>, items: &[T]) {
        let wanted = self.wanted_capacity(items.len(), ctx.frame);
        if !items.is_empty() {
            self.upload(ctx, items, wanted);
        } else if wanted != self.capacity {
            self.capacity = wanted;
            self.buffer = ctx.gpu.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(self.label),
                size: (self.capacity * Self::ITEM_BYTES) as u64,
                usage: self.usage,
                mapped_at_creation: false,
            });
        }
    }

    /// The capacity an upload of `len` items in display frame `frame` writes into: grown to hold
    /// them, or, at the end of the expiry window, stepped down to what the window's peak needs.
    fn wanted_capacity(&mut self, len: usize, frame: u64) -> usize {
        self.peak = self.peak.max(len);
        if !self.window.ends(frame) {
            return self.capacity.max(grown_capacity(len));
        }
        // The frame that ends a window is the window's own; the next begins past it.
        let peak = std::mem::take(&mut self.peak);
        grown_capacity(peak).max(self.initial_capacity)
    }
}

const fn grown_capacity(needed_len: usize) -> usize {
    needed_len.next_power_of_two()
}

#[cfg(test)]
mod tests {
    use crate::gpu::resource::dynamic_buffer::{DynamicBuffer, grown_capacity};
    use crate::gpu::resource::expiry_window::EXPIRY_FRAMES;
    use crate::gpu::test_gpu::headless_test_gpu;

    /// A buffer grows to the next power of two of an upload, keeps it through the expiry window
    /// that upload fell in, steps down to the next window's peak at its end, 10 items to 16, and
    /// to the capacity it was made at once a window uploads nothing, as a feature stops drawing.
    #[test]
    fn a_buffer_steps_down_to_each_windows_peak() {
        let gpu = headless_test_gpu();
        let mut buffer = DynamicBuffer::<[u32; 4]>::vertex(&gpu, "test.decay", 4);
        let upload = |buffer: &mut DynamicBuffer<[u32; 4]>, len: usize| {
            gpu.frame(|ctx| buffer.upload_instances(ctx, &vec![[0; 4]; len]));
        };
        upload(&mut buffer, 1000);
        assert_eq!(buffer.capacity(), 1024);
        for _ in 1..EXPIRY_FRAMES {
            upload(&mut buffer, 10);
        }
        assert_eq!(
            buffer.capacity(),
            1024,
            "the window the 1000 fell in keeps them"
        );
        for _ in 0..EXPIRY_FRAMES {
            upload(&mut buffer, 10);
        }
        assert_eq!(buffer.capacity(), 16, "stepped down to the window's peak");
        for _ in 0..EXPIRY_FRAMES {
            upload(&mut buffer, 0);
        }
        assert_eq!(
            buffer.capacity(),
            4,
            "an idle window steps down to the first capacity"
        );
    }

    #[test]
    fn growth_rounds_to_the_next_power_of_two() {
        assert_eq!(grown_capacity(1), 1);
        assert_eq!(grown_capacity(2), 2);
        assert_eq!(grown_capacity(3), 4);
        assert_eq!(grown_capacity(257), 512);
    }
}
