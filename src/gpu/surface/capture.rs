//! [`Capture`]: a region of a texture on its way back from the device.

use crate::gpu::device::gpu::Gpu;
use crate::gpu::pipeline::blit_pipeline;
use crate::gpu::resource::readback_staging::ReadbackStaging;
use crate::gpu::resource::texture_binding::TextureBinding;
use crate::gpu::texel;
use crate::gpu::texel::TargetSpec;
use crate::primitives::geometry::urect::URect;
use crate::primitives::paint::image::Image;
use std::iter;

const BYTES_PER_PIXEL: u32 = 4;

/// One region of a texture, copied into a staging buffer that maps without blocking the frame;
/// [`Self::take`] hands the pixels over once the device is done. A screenshot copies the
/// backbuffer, a test reads back a target.
#[derive(Debug)]
pub(crate) struct Capture {
    staging: ReadbackStaging,
    /// What [`Self::take`] does to the staged bytes to read them in [`Self::FORMAT`]'s order.
    order: TexelOrder,
}

/// How a texture's bytes reach [`Capture::FORMAT`]'s byte order, the one classifier every
/// read-back asks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TexelOrder {
    /// 8-bit RGBA: as they are.
    Rgba,
    /// 8-bit BGRA: red and blue swapped.
    Bgra,
    /// Any other format: blitted into [`Capture::FORMAT`] first. A frame's target is sRGB or
    /// float, so only a test's read-back meets an 8-bit unorm one, whose bytes it takes as they
    /// are.
    Blit,
}

impl TexelOrder {
    pub(crate) const fn of(format: wgpu::TextureFormat) -> Self {
        match format {
            wgpu::TextureFormat::Rgba8UnormSrgb | wgpu::TextureFormat::Rgba8Unorm => Self::Rgba,
            wgpu::TextureFormat::Bgra8UnormSrgb | wgpu::TextureFormat::Bgra8Unorm => Self::Bgra,
            _ => Self::Blit,
        }
    }

    /// `bytes`, copied in this order, put in RGBA order in place, four to a texel.
    ///
    /// # Panics
    ///
    /// For [`Self::Blit`], whose bytes a blit puts in order.
    pub(crate) fn to_rgba(self, bytes: &mut [u8]) {
        match self {
            Self::Rgba => {}
            Self::Bgra => {
                for texel in bytes.as_chunks_mut::<{ BYTES_PER_PIXEL as usize }>().0 {
                    texel.swap(0, 2);
                }
            }
            Self::Blit => panic!("a blit puts the bytes in order"),
        }
    }
}

/// What a capture copies, and how.
#[derive(Clone, Copy, Debug)]
pub(crate) struct CaptureSource<'a> {
    /// A texture made with `COPY_SRC`.
    pub(crate) texture: &'a wgpu::Texture,
    /// The blit into [`Capture::FORMAT`], where [`TexelOrder::of`] reads the texture's format as
    /// [`TexelOrder::Blit`].
    pub(crate) blit: Option<CaptureBlit<'a>>,
}

/// The blit a capture of a texture not in 8-bit RGBA or BGRA encodes through.
#[derive(Clone, Copy, Debug)]
pub(crate) struct CaptureBlit<'a> {
    pub(crate) pipeline: &'a wgpu::RenderPipeline,
    pub(crate) binding: &'a TextureBinding,
}

impl Capture {
    /// The image a capture always lands in: what an image file holds, whatever the frame's target
    /// format, so a BGRA or float target reads back as every other.
    pub(crate) const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;

    /// Start copying `region` of the source texture, which holds what was submitted before, and
    /// map the copy once the device finishes. An 8-bit texture is copied as is, BGRA swapped on
    /// the way out; any other format is first blitted into a region-sized [`Self::FORMAT`]
    /// texture. Nothing is allocated past the region.
    ///
    /// # Panics
    ///
    /// For a texture made without `COPY_SRC`, and for one that needs a blit when `source` has
    /// none.
    pub(crate) fn start(gpu: &Gpu, source: CaptureSource<'_>, region: URect) -> Self {
        let CaptureSource { texture, blit } = source;
        assert!(
            texture.usage().contains(wgpu::TextureUsages::COPY_SRC),
            "a capture copies out of a texture made with COPY_SRC"
        );
        debug_assert!(
            region.size.x > 0 && region.size.y > 0,
            "an empty region asks no capture: {region:?}"
        );
        let full = texel::size(texture);
        debug_assert!(
            region.min.x + region.size.x <= full.x && region.min.y + region.size.y <= full.y,
            "a capture region {region:?} inside the {full} texture"
        );
        let extent = texel::extent(region.size);
        let staging = ReadbackStaging::new(
            gpu,
            "palantir.capture.staging",
            region.size,
            BYTES_PER_PIXEL,
        );
        let mut encoder = gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("palantir.capture"),
            });
        let at_region = wgpu::TexelCopyTextureInfo {
            texture,
            mip_level: 0,
            origin: texel::origin(region.min),
            aspect: wgpu::TextureAspect::All,
        };
        let into_buffer = staging.copy_info();
        let format = texture.format();
        let order = match TexelOrder::of(format) {
            TexelOrder::Blit => {
                let CaptureBlit { pipeline, binding } =
                    blit.unwrap_or_else(|| panic!("a {format:?} texture needs the capture blit"));
                let texture = |label, format, usage| {
                    texel::texture_2d(
                        gpu,
                        TargetSpec {
                            label,
                            size: region.size,
                            format,
                            usage,
                        },
                    )
                };
                // The blit reads texel for texel at the target's own position, so the region is
                // first copied to a texture of its own size.
                let cut = texture(
                    "palantir.capture.region",
                    format,
                    wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::TEXTURE_BINDING,
                );
                encoder.copy_texture_to_texture(at_region, cut.as_image_copy(), extent);
                let encoded = texture(
                    "palantir.capture.encoded",
                    Self::FORMAT,
                    wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
                );
                let cut_view = cut.create_view(&wgpu::TextureViewDescriptor::default());
                let bind_group =
                    binding.texture_group(gpu, &cut_view, "palantir.capture.region.bg");
                let encoded_view = encoded.create_view(&wgpu::TextureViewDescriptor::default());
                blit_pipeline::draw(&mut encoder, &bind_group, &encoded_view, pipeline);
                encoder.copy_texture_to_buffer(encoded.as_image_copy(), into_buffer, extent);
                TexelOrder::Rgba
            }
            order => {
                encoder.copy_texture_to_buffer(at_region, into_buffer, extent);
                order
            }
        };
        gpu.queue.submit(iter::once(encoder.finish()));
        staging.map_read();
        Self { staging, order }
    }

    /// Whether the copy's map callback has yet to run, which on a lost device it may never do
    /// ([`MapState::is_pending`](crate::gpu::resource::map_state::MapState::is_pending)).
    pub(crate) fn is_pending(&self) -> bool {
        self.staging.map().is_pending()
    }

    /// The pixels, once the device has finished the copy and a poll ran its map callback; `None`
    /// when the copy failed to map, which only a lost device does.
    ///
    /// # Panics
    ///
    /// While the copy [`is_pending`](Self::is_pending).
    pub(crate) fn take(self) -> Option<Image> {
        let map = self.staging.map();
        assert!(!map.is_pending(), "a capture taken before its copy landed");
        if !map.is_mapped() {
            return None;
        }
        let mut pixels = Vec::new();
        self.staging.unpad_into(&mut pixels);
        self.order.to_rgba(&mut pixels);
        Some(
            Image::from_srgba8(self.staging.size(), pixels)
                .expect("a copy of a non-empty region holds four bytes per pixel"),
        )
    }
}

#[cfg(test)]
pub(crate) mod internals {
    use crate::gpu::device::gpu::Gpu;
    use crate::gpu::resource::readback_staging::ReadbackStaging;
    use crate::gpu::surface::capture::{Capture, TexelOrder};

    impl Capture {
        /// A capture whose copy never finishes, as one the device is still running.
        pub(crate) fn never_landing(gpu: &Gpu) -> Self {
            Self {
                staging: ReadbackStaging::never_landing(gpu),
                order: TexelOrder::Rgba,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::gpu::surface::capture::TexelOrder;

    /// 8-bit RGBA reads as it is and BGRA swaps, sRGB or not; every other format, half float
    /// among them, blits. Two BGRA texels swap red and blue each, and RGBA bytes stay.
    #[test]
    fn each_format_reads_back_in_rgba_order() {
        use wgpu::TextureFormat::{
            Bgra8Unorm, Bgra8UnormSrgb, Rgba8Unorm, Rgba8UnormSrgb, Rgba16Float,
        };
        assert_eq!(
            [
                Rgba8Unorm,
                Rgba8UnormSrgb,
                Bgra8Unorm,
                Bgra8UnormSrgb,
                Rgba16Float
            ]
            .map(TexelOrder::of),
            [
                TexelOrder::Rgba,
                TexelOrder::Rgba,
                TexelOrder::Bgra,
                TexelOrder::Bgra,
                TexelOrder::Blit,
            ]
        );
        let mut bytes = [1, 2, 3, 4, 5, 6, 7, 8];
        TexelOrder::Bgra.to_rgba(&mut bytes);
        assert_eq!(bytes, [3, 2, 1, 4, 7, 6, 5, 8]);
        TexelOrder::Rgba.to_rgba(&mut bytes);
        assert_eq!(bytes, [3, 2, 1, 4, 7, 6, 5, 8]);
    }
}
