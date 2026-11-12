//! `DynamicBuffer`: a `wgpu::Buffer` plus power-of-two growth and decay. `upload_parts` grows
//! the buffer when the parts exceed capacity and writes them from offset 0: through the staging belt, or
//! into the new buffer as it is created on a grow frame. At the end of each expiry window it
//! steps down to the window's peak, as `src/gpu/AGENTS.md`'s "Scratch decays" asks of per-frame scratch:
//! one frame of a million glyph quads leaves no 32 MiB buffer for the window's life. No
//! content-hash dedup: a belt memcpy is cheaper than FxHash of the same bytes, so gating by hash
//! is net-negative.
//!
//! Used by every pipeline (`quad`, `mesh`, `image`, `curve`), the `text` backend's vbuf, the blur's
//! params and the backdrop slot table.

use crate::common::expiry_window::ExpiryWindow;
use crate::gpu::device::gpu::{FilledBuffer, Gpu};
use crate::gpu::frame::gpu_ctx::GpuCtx;
use std::marker::PhantomData;
use tinyvec::ArrayVec;

/// The most parts one upload writes: a stream and what follows it.
const PARTS_MAX: usize = 2;

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

    /// Writes `parts` one after another from offset 0 of a buffer of `wanted` items: a new one
    /// where `wanted` moved, created holding the bytes, with no belt staging copy or
    /// `copy_buffer_to_buffer`
    /// ([`Gpu::create_filled_buffer`](crate::gpu::device::gpu::Gpu::create_filled_buffer)).
    fn upload(&mut self, ctx: &mut GpuCtx<'_>, parts: &[&[T]], wanted: usize) {
        if wanted != self.capacity {
            self.capacity = wanted;
            let mut bytes: ArrayVec<[&[u8]; PARTS_MAX]> = ArrayVec::new();
            bytes.extend(parts.iter().map(|part| bytemuck::cast_slice(part)));
            self.buffer = ctx.gpu.create_filled_buffer(
                FilledBuffer {
                    label: self.label,
                    size: (self.capacity * Self::ITEM_BYTES) as u64,
                    usage: self.usage,
                },
                &bytes,
            );
            return;
        }
        let mut at = 0;
        for part in parts.iter().filter(|part| !part.is_empty()) {
            ctx.write(
                &self.buffer,
                (at * Self::ITEM_BYTES) as u64,
                bytemuck::cast_slice(part),
            );
            at += part.len();
        }
    }

    /// Uploads a slice of `Pod` instances to offset 0, as [`Self::upload_parts`] of one part.
    pub(crate) fn upload_instances(&mut self, ctx: &mut GpuCtx<'_>, items: &[T]) {
        self.upload_parts(ctx, &[items]);
    }

    /// Uploads `parts`, at most [`PARTS_MAX`], one after another from offset 0, so one
    /// buffer holds a stream and what follows it. An empty upload writes nothing, and still
    /// counts toward the window's peak, so a buffer a feature stopped using steps down. The
    /// guard, `cast_slice` and count are identical across every instanced pipeline, so they live
    /// here.
    pub(crate) fn upload_parts(&mut self, ctx: &mut GpuCtx<'_>, parts: &[&[T]]) {
        debug_assert!(
            parts.len() <= PARTS_MAX,
            "{} parts to one upload",
            parts.len()
        );
        let len: usize = parts.iter().map(|part| part.len()).sum();
        let wanted = self.wanted_capacity(len, ctx.frame);
        if len != 0 {
            self.upload(ctx, parts, wanted);
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
    use crate::common::expiry_window::EXPIRY_FRAMES;
    use crate::gpu::device::gpu_wait::GpuWait;
    use crate::gpu::resource::dynamic_buffer::{BufferSpec, DynamicBuffer, grown_capacity};
    use crate::gpu::surface::map_state::MapState;
    use crate::gpu::test_gpu::{headless_test_gpu, isolated_headless_test_gpu};

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

    /// A buffer holds its parts back to back, through the grow that makes a new buffer holding
    /// them and through the belt write into a kept one: 3 then 2 items grow a 2-item buffer to 8
    /// and hold `1 2 3 4 5`; 2 then 1 into the same 8 hold `6 7 8`, the rest left as it was.
    #[test]
    fn a_buffer_holds_its_parts_through_a_grow_and_a_write() {
        let gpu = isolated_headless_test_gpu();
        let mut buffer = DynamicBuffer::<u32>::new(
            &gpu,
            BufferSpec {
                label: "test.parts",
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::COPY_SRC,
                initial_capacity: 2,
            },
        );
        let read = |buffer: &DynamicBuffer<u32>| -> Vec<u32> {
            let size = buffer.buffer.size();
            let copy = gpu.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("test.parts.read"),
                size,
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            });
            gpu.frame(|ctx| {
                ctx.encoder
                    .copy_buffer_to_buffer(&buffer.buffer, 0, &copy, 0, size);
            });
            let map = MapState::new();
            map.map_read(&copy);
            assert_eq!(gpu.wait_until(&|| !map.is_pending()), GpuWait::Done);
            bytemuck::cast_slice(&copy.slice(..).get_mapped_range().unwrap()).to_vec()
        };
        gpu.frame(|ctx| buffer.upload_parts(ctx, &[&[1, 2, 3], &[4, 5]]));
        assert_eq!(buffer.capacity(), 8);
        assert_eq!(read(&buffer)[..5], [1, 2, 3, 4, 5]);
        gpu.frame(|ctx| buffer.upload_parts(ctx, &[&[6, 7], &[8]]));
        assert_eq!(buffer.capacity(), 8, "kept");
        assert_eq!(read(&buffer)[..5], [6, 7, 8, 4, 5]);
    }

    #[test]
    fn growth_rounds_to_the_next_power_of_two() {
        assert_eq!(grown_capacity(1), 1);
        assert_eq!(grown_capacity(2), 2);
        assert_eq!(grown_capacity(3), 4);
        assert_eq!(grown_capacity(257), 512);
    }
}
