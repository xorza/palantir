//! The one `queue.write_texture` seam and its per-frame counters. The image
//! store and gradient atlas write through [`TextureRegion::write`], so a third
//! uploader cannot skip the tally; the glyph atlas batches through
//! `copy_buffer_to_texture` instead and is not counted. Counting is behind the
//! `internals` feature. Not a wrapper around `wgpu::Queue`: it could not make the
//! seam unbypassable, as [`GpuFrameContext`] hands app code the raw queue.
//!
//! [`GpuFrameContext`]: crate::gpu::device::gpu_frame_context::GpuFrameContext

use glam::UVec2;

/// A destination band in a 2D texture: mip 0, one layer, full width, so `first_row` is the only origin.
#[derive(Clone, Copy, Debug)]
pub(super) struct TextureRegion<'a> {
    pub(super) texture: &'a wgpu::Texture,
    pub(super) first_row: u32,
    pub(super) size: UVec2,
    /// Source stride, carried because the uploaders' texel widths differ. A
    /// multiple of `COPY_BYTES_PER_ROW_ALIGNMENT` reaches the texture in one
    /// copy; other pitches are re-packed inside wgpu. Padding here to avoid that
    /// is a pessimisation, as wgpu still copies the whole into staging.
    pub(super) bytes_per_row: u32,
}

impl TextureRegion<'_> {
    /// [`wgpu::Queue::write_texture`] into this region; `data` is row-major at [`Self::bytes_per_row`].
    pub(super) fn write(self, queue: &wgpu::Queue, data: &[u8]) {
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: self.texture,
                mip_level: 0,
                origin: wgpu::Origin3d {
                    x: 0,
                    y: self.first_row,
                    z: 0,
                },
                aspect: wgpu::TextureAspect::All,
            },
            data,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(self.bytes_per_row),
                rows_per_image: Some(self.size.y),
            },
            wgpu::Extent3d {
                width: self.size.x,
                height: self.size.y,
                depth_or_array_layers: 1,
            },
        );
    }
}
