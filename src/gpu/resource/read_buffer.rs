//! [`ReadBuffer`]: a buffer the CPU reads back, kept with the state of its read map.

use crate::gpu::device::gpu::Gpu;
use crate::gpu::resource::map_state::MapState;

/// A `MAP_READ` buffer a copy fills, and where its read map stands: the two travel together, so
/// no reader pairs a buffer with another's state.
#[derive(Debug)]
pub(crate) struct ReadBuffer {
    buffer: wgpu::Buffer,
    map: MapState,
}

impl ReadBuffer {
    /// A buffer of `size` bytes a copy writes and the CPU maps for reading.
    pub(crate) fn new(gpu: &Gpu, label: &'static str, size: u64) -> Self {
        Self {
            buffer: gpu.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size,
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            }),
            map: MapState::new(),
        }
    }

    /// The buffer, for the copy that fills it.
    pub(crate) const fn buffer(&self) -> &wgpu::Buffer {
        &self.buffer
    }

    pub(crate) const fn map(&self) -> &MapState {
        &self.map
    }

    /// Start mapping the buffer for reading, once the submit that fills it is made.
    pub(crate) fn map_read(&self) {
        self.map.map_read(&self.buffer);
    }

    /// Hand the mapped bytes to `read`, then unmap, leaving the map idle for the next read.
    ///
    /// # Panics
    ///
    /// Unless the map [landed](MapState::is_mapped).
    pub(crate) fn read<R>(&self, read: impl FnOnce(&[u8]) -> R) -> R {
        assert!(self.map.is_mapped(), "a read buffer reads once mapped");
        let value = {
            let view = self
                .buffer
                .slice(..)
                .get_mapped_range()
                .expect("a mapped read buffer reads whole");
            read(&view)
        };
        self.buffer.unmap();
        self.map.clear();
        value
    }

    /// Drop whatever map is in flight or landed, leaving the map idle: a pending map is aborted,
    /// a landed one unmapped, a failed one forgotten.
    pub(crate) fn discard(&self) {
        if self.map.is_pending() || self.map.is_mapped() {
            self.buffer.unmap();
        }
        self.map.clear();
    }
}

#[cfg(test)]
pub(crate) mod internals {
    use crate::gpu::device::gpu::Gpu;
    use crate::gpu::resource::map_state::MapState;
    use crate::gpu::resource::read_buffer::ReadBuffer;

    impl ReadBuffer {
        /// A buffer of `size` bytes whose map never lands, as one the device is still copying for.
        pub(crate) fn never_landing(gpu: &Gpu, size: u64) -> Self {
            Self {
                map: MapState::never_landing(),
                ..Self::new(gpu, "palantir.read_buffer.test", size)
            }
        }
    }
}
