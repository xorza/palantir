//! Per-frame GPU handles bundled so callers thread one `&mut GpuCtx`, built right after the main
//! encoder; dropping it releases the borrows.

use crate::gpu::device::gpu::Gpu;
use crate::gpu::device::upload_staging::{StagedBytes, UploadStaging};
use crate::gpu::texel;
use glam::UVec2;

#[derive(Debug)]
pub(crate) struct GpuCtx<'a> {
    pub(crate) gpu: &'a Gpu,
    staging: &'a mut UploadStaging,
    pub(crate) encoder: &'a mut wgpu::CommandEncoder,
    /// The backend's display frame, one round in which each live render owner submits once:
    /// the clock every piece of per-frame scratch ages against
    /// ([`ExpiryWindow`](crate::gpu::resource::expiry_window::ExpiryWindow)).
    pub(crate) frame: u64,
}

impl<'a> GpuCtx<'a> {
    pub(crate) const fn new(
        gpu: &'a Gpu,
        staging: &'a mut UploadStaging,
        encoder: &'a mut wgpu::CommandEncoder,
        frame: u64,
    ) -> Self {
        Self {
            gpu,
            staging,
            encoder,
            frame,
        }
    }

    /// Schedule a belt-backed copy from staging to `dst@offset`. Empty `bytes` is a no-op; `offset` and length must be multiples of `COPY_BUFFER_ALIGNMENT` (4).
    pub(crate) fn write(&mut self, dst: &wgpu::Buffer, offset: u64, bytes: &[u8]) {
        let Some(mut view) = self.write_view(dst, offset, bytes.len() as u64) else {
            return;
        };
        view.copy_from_slice(bytes);
    }

    /// `bytes` of mapped belt staging at an offset aligned to `alignment`, for a copy the caller
    /// records itself, as into a texture, which [`Self::write`] cannot reach; `bytes` must be a
    /// multiple of `COPY_BUFFER_ALIGNMENT` (4) and `alignment` a power of two.
    ///
    /// `None` for no bytes, and on a lost device, whose belt allocations do not map: the loss can
    /// come inside the frame, before any call reports it, and the copy would be dropped as every
    /// call on a lost device is.
    pub(crate) fn stage(&mut self, bytes: u64, alignment: u64) -> Option<Staged<'_>> {
        let size = wgpu::BufferSize::new(bytes)?;
        debug_assert!(
            alignment.is_power_of_two(),
            "a staging alignment is a power of two, not {alignment}"
        );
        let alignment = wgpu::BufferSize::new(alignment).expect("a power of two is not zero");
        let StagedBytes {
            view,
            buffer,
            offset,
        } = self.staging.allocate(&self.gpu.device, size, alignment)?;
        Some(Staged {
            view,
            buffer,
            offset,
            encoder: self.encoder,
        })
    }

    /// Stage every copy's rows in the belt once, each at the pitch a buffer-to-texture copy reads
    /// ([`RowCopy::pitch`]), and record one copy a rectangle on the frame's encoder, ahead of
    /// every pass. Padding bytes keep the belt chunk's old contents, which no copy reads. A lost
    /// device declines the staging, and then nothing is copied.
    ///
    /// Not `queue.write_texture`: queue writes run before every command buffer in the submit,
    /// under a copy this encoder recorded first, and wgpu allocates a staging buffer per call.
    pub(crate) fn copy_rows<'r>(&mut self, copies: impl Iterator<Item = RowCopy<'r>> + Clone) {
        let bytes: usize = copies
            .clone()
            .map(|copy| copy.pitch() * copy.size.y as usize)
            .sum();
        let Some(Staged {
            mut view,
            buffer,
            offset,
            encoder,
        }) = self.stage(bytes as u64, u64::from(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT))
        else {
            return;
        };
        let mut at = 0;
        for copy in copies {
            let (row_bytes, pitch) = (copy.row_bytes(), copy.pitch());
            encoder.copy_buffer_to_texture(
                wgpu::TexelCopyBufferInfo {
                    buffer,
                    layout: wgpu::TexelCopyBufferLayout {
                        offset: offset + at as u64,
                        bytes_per_row: Some(pitch as u32),
                        rows_per_image: Some(copy.size.y),
                    },
                },
                wgpu::TexelCopyTextureInfo {
                    texture: copy.texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d {
                        x: copy.origin.x,
                        y: copy.origin.y,
                        z: 0,
                    },
                    aspect: wgpu::TextureAspect::All,
                },
                texel::extent(copy.size),
            );
            if row_bytes == pitch {
                view.slice(at..at + copy.rows.len())
                    .copy_from_slice(copy.rows);
                at += copy.rows.len();
            } else {
                for row in copy.rows.chunks_exact(row_bytes) {
                    view.slice(at..at + row_bytes).copy_from_slice(row);
                    at += pitch;
                }
            }
        }
    }

    /// [`Self::write`] without the source slice: the mapped staging bytes, for a caller composing in place (one memcpy, not two). Unwritten bytes keep the belt chunk's old contents and must never be read. `None` as [`Self::stage`] gives it.
    ///
    /// Not `StagingBelt::write_buffer`, which panics where an allocation does not map.
    pub(crate) fn write_view(
        &mut self,
        dst: &wgpu::Buffer,
        offset: u64,
        bytes: u64,
    ) -> Option<wgpu::BufferViewMut> {
        debug_assert!(
            offset.is_multiple_of(wgpu::COPY_BUFFER_ALIGNMENT),
            "a buffer copy starts on a multiple of `COPY_BUFFER_ALIGNMENT`"
        );
        debug_assert!(
            bytes.is_multiple_of(wgpu::COPY_BUFFER_ALIGNMENT),
            "a buffer copy is a multiple of `COPY_BUFFER_ALIGNMENT` long"
        );
        let Staged {
            view,
            buffer,
            offset: from,
            encoder,
        } = self.stage(bytes, wgpu::COPY_BUFFER_ALIGNMENT)?;
        encoder.copy_buffer_to_buffer(buffer, from, dst, offset, bytes);
        Some(view)
    }
}

/// One rectangle of a texture that [`GpuCtx::copy_rows`] fills: `rows` holds its `size.y` rows
/// packed with no slack between them.
#[derive(Clone, Copy, Debug)]
pub(crate) struct RowCopy<'a> {
    pub(crate) texture: &'a wgpu::Texture,
    pub(crate) origin: UVec2,
    pub(crate) size: UVec2,
    pub(crate) rows: &'a [u8],
}

impl RowCopy<'_> {
    /// Bytes of one packed row.
    fn row_bytes(self) -> usize {
        debug_assert!(self.size.y > 0, "a copy covers a row");
        debug_assert!(
            self.rows.len().is_multiple_of(self.size.y as usize),
            "a copy's bytes are whole rows"
        );
        self.rows.len() / self.size.y as usize
    }

    /// The pitch a buffer-to-texture copy reads rows at: the row padded to
    /// `COPY_BYTES_PER_ROW_ALIGNMENT`.
    fn pitch(self) -> usize {
        self.row_bytes()
            .next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT as usize)
    }
}

/// Belt staging [`GpuCtx::stage`] handed out: the mapped bytes, where they sit, and the encoder
/// the copies out of them record on, submitted with the belt's own.
#[derive(Debug)]
pub(crate) struct Staged<'a> {
    pub(crate) view: wgpu::BufferViewMut,
    pub(crate) buffer: &'a wgpu::Buffer,
    pub(crate) offset: u64,
    pub(crate) encoder: &'a mut wgpu::CommandEncoder,
}

#[cfg(test)]
mod tests {
    use crate::gpu::frame::gpu_ctx::RowCopy;
    use crate::gpu::surface::owned_target::TargetSpec;
    use crate::gpu::test_gpu::headless_test_gpu;
    use crate::gpu::texel;
    use glam::UVec2;

    /// Two copies into a 4 × 3 `R8Unorm` texture: 3 × 2 at the origin, rows 3 bytes against the
    /// 256-byte pitch, which the staging pads, and 2 × 1 at (1, 2) after it. Each texel lands as
    /// packed, and the texel no copy covers keeps the zero the texture starts at.
    #[test]
    fn copied_rows_land_at_their_rect_through_the_pitch() {
        let gpu = headless_test_gpu();
        let texture = texel::texture_2d(
            &gpu,
            TargetSpec {
                label: "copy_rows",
                size: UVec2::new(4, 3),
                format: wgpu::TextureFormat::R8Unorm,
                usage: wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::COPY_SRC,
            },
        );
        gpu.frame(|ctx| {
            ctx.copy_rows(
                [
                    RowCopy {
                        texture: &texture,
                        origin: UVec2::ZERO,
                        size: UVec2::new(3, 2),
                        rows: &[1, 2, 3, 4, 5, 6],
                    },
                    RowCopy {
                        texture: &texture,
                        origin: UVec2::new(1, 2),
                        size: UVec2::new(2, 1),
                        rows: &[7, 8],
                    },
                ]
                .into_iter(),
            );
        });
        assert_eq!(
            gpu.read_texture(&texture),
            [1, 2, 3, 0, 4, 5, 6, 0, 0, 7, 8, 0]
        );
    }
}
