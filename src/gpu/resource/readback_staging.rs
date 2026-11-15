//! [`ReadbackStaging`]: a buffer a texture region is copied into and read back from.

use crate::gpu::device::gpu::Gpu;
use crate::gpu::resource::map_state::MapState;
use crate::gpu::resource::read_buffer::ReadBuffer;
use glam::UVec2;

/// A buffer that holds `size` texels of `texel` bytes, its rows padded to whole
/// `COPY_BYTES_PER_ROW_ALIGNMENT` units as a texture-to-buffer copy lays them, and the state of its
/// read map. [`Capture`](crate::gpu::surface::capture::Capture) and
/// [`TexelReadback`](crate::gpu::surface::texel_readback::TexelReadback) read back through it.
#[derive(Debug)]
pub(crate) struct ReadbackStaging {
    read: ReadBuffer,
    size: UVec2,
    /// One row's bytes.
    row: u32,
    /// A row's stride in the buffer.
    padded_row: u32,
}

impl ReadbackStaging {
    pub(crate) fn new(gpu: &Gpu, label: &'static str, size: UVec2, texel: u32) -> Self {
        let row = size.x * texel;
        let padded_row = row.next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
        Self {
            read: ReadBuffer::new(gpu, label, u64::from(padded_row) * u64::from(size.y)),
            size,
            row,
            padded_row,
        }
    }

    /// The buffer as a texture-to-buffer copy writes it.
    pub(crate) const fn copy_info(&self) -> wgpu::TexelCopyBufferInfo<'_> {
        wgpu::TexelCopyBufferInfo {
            buffer: self.read.buffer(),
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(self.padded_row),
                rows_per_image: Some(self.size.y),
            },
        }
    }

    /// Start mapping the buffer for reading, once the submit that fills it is made.
    pub(crate) fn map_read(&self) {
        self.read.map_read();
    }

    pub(crate) const fn map(&self) -> &MapState {
        self.read.map()
    }

    /// The texels the buffer holds, per side.
    pub(crate) const fn size(&self) -> UVec2 {
        self.size
    }

    /// Abort a map still in flight, so the next can map the buffer again.
    pub(crate) fn abort(&self) {
        self.read.discard();
    }

    /// The mapped rows into `out`, top down with the padding dropped, then unmap, leaving the map
    /// idle for the next read.
    ///
    /// # Panics
    ///
    /// Unless the map [landed](MapState::is_mapped).
    pub(crate) fn unpad_into(&self, out: &mut Vec<u8>) {
        out.clear();
        self.read.read(|view| {
            out.reserve_exact((self.row * self.size.y) as usize);
            for line in view.chunks_exact(self.padded_row as usize) {
                out.extend_from_slice(&line[..self.row as usize]);
            }
        });
    }
}

#[cfg(test)]
pub(crate) mod internals {
    use crate::gpu::device::gpu::Gpu;
    use crate::gpu::resource::read_buffer::ReadBuffer;
    use crate::gpu::resource::readback_staging::ReadbackStaging;
    use glam::UVec2;

    impl ReadbackStaging {
        /// A one-texel staging whose map never lands, as one the device is still running.
        pub(crate) fn never_landing(gpu: &Gpu) -> Self {
            let staging = Self::new(gpu, "palantir.staging.test", UVec2::ONE, 4);
            Self {
                read: ReadBuffer::never_landing(gpu, staging.read.buffer().size()),
                ..staging
            }
        }
    }
}
