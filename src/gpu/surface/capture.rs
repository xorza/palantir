//! [`Capture`]: a region of a texture on its way back from the device.

use crate::gpu::device::gpu::Gpu;
use crate::gpu::pipeline::blit_pipeline;
use crate::gpu::resource::texture_binding::TextureBinding;
use crate::gpu::surface::render_target;
use crate::gpu::surface::staging::Staging;
use crate::primitives::geometry::urect::URect;
use crate::primitives::paint::image::Image;
use glam::UVec2;
use std::iter;

const BYTES_PER_PIXEL: u32 = 4;

/// One region of a texture, copied into a staging buffer that maps without blocking the frame;
/// [`Self::take`] hands the pixels over once the device is done. A screenshot copies the
/// backbuffer, a test reads back a target.
#[derive(Debug)]
pub(crate) struct Capture {
    staging: Staging,
    size: UVec2,
    /// The staged bytes are BGRA, which [`Self::take`] swaps into [`Self::FORMAT`]'s order.
    bgra: bool,
}

/// What a capture copies, and how.
#[derive(Clone, Copy, Debug)]
pub(crate) struct CaptureSource<'a> {
    /// A texture made with `COPY_SRC`.
    pub(crate) texture: &'a wgpu::Texture,
    /// The blit into [`Capture::FORMAT`], where [`Capture::needs_blit`] says the texture asks
    /// for one.
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

    /// Whether a texture of `format` reaches [`Self::FORMAT`] only through a blit: any but the
    /// 8-bit RGBA and BGRA formats, whose bytes a capture copies as they are. A frame's target
    /// is sRGB or float, so only a test's read-back meets an 8-bit unorm one, whose bytes it takes
    /// as they are.
    pub(crate) const fn needs_blit(format: wgpu::TextureFormat) -> bool {
        !matches!(
            format,
            wgpu::TextureFormat::Rgba8UnormSrgb
                | wgpu::TextureFormat::Bgra8UnormSrgb
                | wgpu::TextureFormat::Rgba8Unorm
                | wgpu::TextureFormat::Bgra8Unorm
        )
    }

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
        let full = render_target::texture_size(texture);
        debug_assert!(
            region.min.x + region.size.x <= full.x && region.min.y + region.size.y <= full.y,
            "a capture region {region:?} inside the {full} texture"
        );
        let extent = render_target::extent(region.size);
        let staging = Staging::new(
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
            origin: render_target::origin(region.min),
            aspect: wgpu::TextureAspect::All,
        };
        let into_buffer = staging.copy_info();
        let format = texture.format();
        let bgra = matches!(
            format,
            wgpu::TextureFormat::Bgra8UnormSrgb | wgpu::TextureFormat::Bgra8Unorm
        );
        if Self::needs_blit(format) {
            let CaptureBlit { pipeline, binding } =
                blit.unwrap_or_else(|| panic!("a {format:?} texture needs the capture blit"));
            let texture = |label, format, usage| {
                gpu.device.create_texture(&wgpu::TextureDescriptor {
                    label: Some(label),
                    size: extent,
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format,
                    usage,
                    view_formats: &[],
                })
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
            let bind_group = binding.bind_group(gpu, &cut_view, "palantir.capture.region.bg");
            let encoded_view = encoded.create_view(&wgpu::TextureViewDescriptor::default());
            blit_pipeline::draw(&mut encoder, &bind_group, &encoded_view, pipeline);
            encoder.copy_texture_to_buffer(encoded.as_image_copy(), into_buffer, extent);
        } else {
            encoder.copy_texture_to_buffer(at_region, into_buffer, extent);
        }
        gpu.queue.submit(iter::once(encoder.finish()));
        staging.map_read();
        Self {
            staging,
            size: region.size,
            bgra,
        }
    }

    /// Whether the copy's map callback has yet to run, which on a lost device it may never do
    /// ([`MapState::is_pending`]).
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
        if self.bgra {
            swap_bgra(&mut pixels);
        }
        Some(
            Image::from_srgba8(self.size, pixels)
                .expect("a copy of a non-empty region holds four bytes per pixel"),
        )
    }
}

/// BGRA bytes swapped into RGBA order in place, four to a texel.
pub(crate) fn swap_bgra(bytes: &mut [u8]) {
    for texel in bytes.as_chunks_mut::<{ BYTES_PER_PIXEL as usize }>().0 {
        texel.swap(0, 2);
    }
}

#[cfg(test)]
pub(crate) mod internals {
    use crate::gpu::device::gpu::Gpu;
    use crate::gpu::surface::capture::Capture;
    use crate::gpu::surface::staging::Staging;
    use glam::UVec2;

    impl Capture {
        /// A capture whose copy never finishes, as one the device is still running.
        pub(crate) fn never_landing(gpu: &Gpu) -> Self {
            Self {
                staging: Staging::never_landing(gpu),
                size: UVec2::ONE,
                bgra: false,
            }
        }
    }
}
