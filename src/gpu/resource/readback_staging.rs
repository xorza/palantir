//! [`ReadbackStaging`]: a buffer a texture region is copied into and read back from.

use crate::gpu::device::gpu::Gpu;
use crate::gpu::resource::map_state::MapState;
use glam::UVec2;

/// A buffer that holds `size` texels of `texel` bytes, its rows padded to whole
/// `COPY_BYTES_PER_ROW_ALIGNMENT` units as a texture-to-buffer copy lays them, and the state of its
/// read map. [`Capture`](crate::gpu::surface::capture::Capture) and
/// [`TexelReadback`](crate::gpu::surface::texel_readback::TexelReadback) read back through it.
#[derive(Debug)]
pub(crate) struct ReadbackStaging {
    buffer: wgpu::Buffer,
    size: UVec2,
    /// One row's bytes.
    row: u32,
    /// A row's stride in the buffer.
    padded_row: u32,
    map: MapState,
}

impl ReadbackStaging {
    pub(crate) fn new(gpu: &Gpu, label: &'static str, size: UVec2, texel: u32) -> Self {
        let row = size.x * texel;
        let padded_row = row.next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
        let buffer = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(label),
            size: u64::from(padded_row) * u64::from(size.y),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        Self {
            buffer,
            size,
            row,
            padded_row,
            map: MapState::new(),
        }
    }

    /// The buffer as a texture-to-buffer copy writes it.
    pub(crate) const fn copy_info(&self) -> wgpu::TexelCopyBufferInfo<'_> {
        wgpu::TexelCopyBufferInfo {
            buffer: &self.buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(self.padded_row),
                rows_per_image: Some(self.size.y),
            },
        }
    }

    /// Start mapping the buffer for reading, once the submit that fills it is made.
    pub(crate) fn map_read(&self) {
        self.map.map_read(&self.buffer);
    }

    pub(crate) const fn map(&self) -> &MapState {
        &self.map
    }

    /// The texels the buffer holds, per side.
    pub(crate) const fn size(&self) -> UVec2 {
        self.size
    }

    /// Abort a map still in flight, so the next can map the buffer again.
    pub(crate) fn abort(&self) {
        if self.map.is_pending() {
            self.buffer.unmap();
        }
    }

    /// The mapped rows into `out`, top down with the padding dropped, then unmap, leaving the map
    /// idle for the next read.
    ///
    /// # Panics
    ///
    /// Unless the map [landed](MapState::is_mapped).
    pub(crate) fn unpad_into(&self, out: &mut Vec<u8>) {
        assert!(self.map.is_mapped(), "a staging buffer reads once mapped");
        out.clear();
        {
            let view = self
                .buffer
                .slice(..)
                .get_mapped_range()
                .expect("a mapped staging buffer reads whole");
            out.reserve_exact((self.row * self.size.y) as usize);
            for line in view.chunks_exact(self.padded_row as usize) {
                out.extend_from_slice(&line[..self.row as usize]);
            }
        }
        self.buffer.unmap();
        self.map.clear();
    }
}

#[cfg(test)]
pub(crate) mod internals {
    use crate::gpu::device::gpu::Gpu;
    use crate::gpu::resource::map_state::MapState;
    use crate::gpu::resource::readback_staging::ReadbackStaging;
    use glam::UVec2;

    impl ReadbackStaging {
        /// A one-texel staging whose map never lands, as one the device is still running.
        pub(crate) fn never_landing(gpu: &Gpu) -> Self {
            Self {
                map: MapState::never_landing(),
                ..Self::new(gpu, "palantir.staging.test", UVec2::ONE, 4)
            }
        }
    }
}
