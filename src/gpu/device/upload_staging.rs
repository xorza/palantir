//! [`UploadStaging`]: where a frame's uploads stage before the encoder copies them out.

use wgpu::util::StagingBelt;

/// A frame's upload staging: the staging belt for the uploads a chunk holds, and a buffer of its
/// own for each larger one, dropped once the frame is submitted. wgpu's belt makes a chunk as
/// large as an allocation that passes its chunk size and keeps every chunk it recalls for its
/// life, so one frame's 32 MiB upload would stay mapped memory for the window's life; AGENTS.md's
/// "Scratch decays" refuses that. Refused: rebuilding the belt after it sits idle, which still
/// keeps the high-water mark for a window of frames and drops the small chunks every frame needs.
#[derive(Debug)]
pub(crate) struct UploadStaging {
    belt: StagingBelt,
    /// The belt's chunk size: an allocation past it stages in a buffer of its own.
    chunk_bytes: u64,
    /// This frame's buffers for allocations past `chunk_bytes`, mapped until [`Self::finish`].
    oversize: Vec<wgpu::Buffer>,
}

/// Bytes [`UploadStaging::allocate`] mapped, and where they sit.
#[derive(Debug)]
pub(crate) struct StagedBytes<'a> {
    pub(crate) view: wgpu::BufferViewMut,
    pub(crate) buffer: &'a wgpu::Buffer,
    pub(crate) offset: u64,
}

impl UploadStaging {
    /// Staging over `device` whose belt allocates chunks of `chunk_bytes`.
    pub(crate) fn new(device: &wgpu::Device, chunk_bytes: u64) -> Self {
        Self {
            belt: StagingBelt::new(device.clone(), chunk_bytes),
            chunk_bytes,
            oversize: Vec::new(),
        }
    }

    /// `size` mapped bytes at an offset aligned to `alignment`: from the belt where a chunk holds
    /// them, else in a buffer of their own. `None` on a lost device, whose staging does not map.
    pub(crate) fn allocate(
        &mut self,
        device: &wgpu::Device,
        size: wgpu::BufferSize,
        alignment: wgpu::BufferSize,
    ) -> Option<StagedBytes<'_>> {
        if size.get() <= self.chunk_bytes {
            let slice = self.belt.allocate(size, alignment);
            let view = slice.get_mapped_range_mut().ok()?;
            return Some(StagedBytes {
                view,
                buffer: slice.buffer(),
                offset: slice.offset(),
            });
        }
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("palantir.upload.oversize"),
            size: size.get(),
            usage: wgpu::BufferUsages::MAP_WRITE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: true,
        });
        // Mapped before it is kept: a lost device's buffer maps nothing, and an unmap of it would
        // be a call on a buffer that is not mapped.
        let view = buffer.slice(..).get_mapped_range_mut().ok()?;
        self.oversize.push(buffer);
        let buffer = self.oversize.last().expect("pushed above");
        Some(StagedBytes {
            view,
            buffer,
            offset: 0,
        })
    }

    /// Close the frame's staging for its submit: unmap the belt's chunks and the oversize
    /// buffers, which the commands copying out of them keep alive past their drop here. For a
    /// bench that recalls on its own; the backend recalls on submit.
    #[cfg(feature = "internals")]
    pub(crate) fn finish(&mut self) {
        self.belt.finish();
        self.unmap_oversize();
    }

    /// Take the belt's chunks back once the frame's work is done.
    #[cfg(feature = "internals")]
    pub(crate) fn recall(&mut self) {
        self.belt.recall();
    }

    /// [`Self::finish`], and recall the belt's chunks when the submit that `encoder` is part of
    /// completes.
    pub(crate) fn finish_and_recall_on_submit(&mut self, encoder: &wgpu::CommandEncoder) {
        self.unmap_oversize();
        self.belt.finish_and_recall_on_submit(encoder);
    }

    /// Unmap this frame's oversize buffers and drop them; the commands copying out of them keep
    /// them alive.
    fn unmap_oversize(&mut self) {
        for buffer in self.oversize.drain(..) {
            buffer.unmap();
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::gpu::device::upload_staging::UploadStaging;
    use crate::gpu::test_gpu::headless_test_gpu;

    /// An allocation a chunk holds stages in the belt; one past it in a buffer of its own, of its
    /// size, which the frame's close unmaps and drops.
    #[test]
    fn an_upload_past_the_chunk_stages_in_its_own_buffer() {
        let gpu = headless_test_gpu();
        let device = &gpu.device;
        let mut staging = UploadStaging::new(device, 1024);
        let size = |bytes| wgpu::BufferSize::new(bytes).unwrap();
        let align = size(wgpu::COPY_BUFFER_ALIGNMENT);
        drop(
            staging
                .allocate(device, size(1024), align)
                .expect("a healthy device maps"),
        );
        assert!(staging.oversize.is_empty(), "a chunk holds 1024 bytes");
        let staged = staging
            .allocate(device, size(4096), align)
            .expect("a healthy device maps");
        assert_eq!((staged.offset, staged.buffer.size()), (0, 4096));
        drop(staged);
        assert_eq!(staging.oversize.len(), 1);
        staging.finish();
        assert!(staging.oversize.is_empty(), "the close drops it");
        staging.recall();
    }
}
