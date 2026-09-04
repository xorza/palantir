//! The one `queue.write_texture` seam, and the per-frame counters behind
//! it.
//!
//! Every `queue.write_texture` in the backend goes through
//! [`TextureRegion::write`] — the image registry and the gradient atlas —
//! so the tally has one place to live and a third uploader cannot quietly
//! skip it. Not every texture *upload*: the glyph atlas batches its
//! pixels through one `copy_buffer_to_texture` on the frame encoder
//! (`raster_atlas::flush_pending_uploads`), which is the shape this
//! counts the absence of. The counting half is gated behind the `bench`
//! feature: the frame bench's per-frame dump is its only reader, and a
//! plain build must not pay two atomic RMWs per texture write.
//!
//! Deliberately not a wrapper around `wgpu::Queue`. One would have to
//! publish `submit`, `get_timestamp_period` and `Clone` as passthroughs
//! to carry the single method that does anything, and it could not make
//! the seam unbypassable either: [`GpuFrameCtx`] hands app code the raw
//! `wgpu::Queue` on purpose, so the raw handle is reachable by design.
//! What the call sites wanted was convenience over the *destination*,
//! which is what this type is.
//!
//! [`GpuFrameCtx`]: crate::renderer::gpu_paint::gpu_frame_ctx::GpuFrameCtx

use glam::UVec2;

/// A destination band in a 2D texture: the whole upload shape the
/// backend has. Mip 0, full aspect, one layer — spelled once here
/// instead of three wgpu descriptors per call site.
///
/// Full-width by construction: both uploaders write whole rows, so there
/// is no x offset to carry and `first_row` is the only origin a caller
/// picks.
#[derive(Clone, Copy, Debug)]
pub(super) struct TextureRegion<'a> {
    pub(super) texture: &'a wgpu::Texture,
    /// Row this band starts at.
    pub(super) first_row: u32,
    /// Extent in texels.
    pub(super) size: UVec2,
    /// Source stride. Carried rather than derived from `size.x`: the two
    /// uploaders have different texel widths (`Rgba8` against
    /// `RgbaF16`).
    ///
    /// A pitch that is already a multiple of
    /// `COPY_BYTES_PER_ROW_ALIGNMENT` reaches the texture in one copy.
    /// Any other pitch is legal and costs a row-by-row re-pack inside
    /// wgpu, which is what an image of arbitrary width pays here — the
    /// gradient atlas's pitch is aligned by construction, so only the
    /// image side meets it.
    ///
    /// **Padding on this side to buy that copy back is a
    /// pessimisation.** It is the same row-by-row fill, plus a buffer of
    /// our own, and wgpu then copies the padded whole a second time into
    /// staging it allocates either way. An image uploads once, at
    /// registration (`ImageTextures::drain_registry`), so the re-pack is
    /// paid per image and never per frame.
    pub(super) bytes_per_row: u32,
}

impl TextureRegion<'_> {
    /// Counted [`wgpu::Queue::write_texture`] into this region. `data` is
    /// the region's texels, row-major at [`Self::bytes_per_row`]; its
    /// length is the recorded upload size.
    pub(super) fn write(self, queue: &wgpu::Queue, data: &[u8]) {
        #[cfg(feature = "bench")]
        counters::note(data.len() as u64);
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

#[cfg(feature = "bench")]
pub(crate) mod counters {
    use std::sync::atomic::{AtomicU64, Ordering::Relaxed};

    static TEXTURE_CALLS: AtomicU64 = AtomicU64::new(0);
    static TEXTURE_BYTES: AtomicU64 = AtomicU64::new(0);

    /// Tally one [`super::TextureRegion::write`] of `bytes`.
    pub(super) fn note(bytes: u64) {
        TEXTURE_CALLS.fetch_add(1, Relaxed);
        TEXTURE_BYTES.fetch_add(bytes, Relaxed);
    }

    /// One frame's worth of [`super::TextureRegion::write`] traffic.
    ///
    /// `pub(crate)` where [`super::TextureRegion`] is `pub(super)`: only
    /// the backend uploaders build a region, but the frame bench — which
    /// lives outside this module tree — reads the tally.
    #[derive(Default, Debug, Clone, Copy)]
    pub(crate) struct WriteStats {
        pub(crate) texture_calls: u64,
        pub(crate) texture_bytes: u64,
    }

    impl WriteStats {
        /// Snapshot the counters and reset them to zero. Call between bench
        /// iters (or between frames in an instrumented harness) to get
        /// per-frame numbers.
        pub(crate) fn take() -> Self {
            Self {
                texture_calls: TEXTURE_CALLS.swap(0, Relaxed),
                texture_bytes: TEXTURE_BYTES.swap(0, Relaxed),
            }
        }
    }
}
