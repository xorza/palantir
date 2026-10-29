//! [`Capture`]: a region of a texture on its way back from the device.

use crate::gpu::device::gpu::Gpu;
use crate::gpu::pipeline::blit_pipeline;
use crate::gpu::resource::texture_binding::TextureBinding;
use crate::gpu::surface::render_target;
use crate::primitives::geometry::urect::URect;
use crate::primitives::paint::image::Image;
use glam::UVec2;
use std::iter;
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};
use std::thread;

const BYTES_PER_PIXEL: u32 = 4;

const MAP_PENDING: u8 = 0;
const MAP_DONE: u8 = 1;
const MAP_FAILED: u8 = 2;

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

#[derive(Debug)]
struct Staging {
    buffer: wgpu::Buffer,
    /// The copy's row stride: whole `COPY_BYTES_PER_ROW_ALIGNMENT` units.
    padded_row: u32,
    /// Written by the map callback, read by [`Capture::take`]; the callback runs inside a device
    /// poll, on whichever thread polls.
    state: Arc<AtomicU8>,
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
    pub(crate) fn start(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        source: CaptureSource<'_>,
        region: URect,
    ) -> Self {
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
        let padded_row = (region.size.x * BYTES_PER_PIXEL)
            .div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT)
            * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("palantir.capture.staging"),
            size: u64::from(padded_row) * u64::from(region.size.y),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("palantir.capture"),
        });
        let at_region = wgpu::TexelCopyTextureInfo {
            texture,
            mip_level: 0,
            origin: render_target::origin(region.min),
            aspect: wgpu::TextureAspect::All,
        };
        let into_buffer = wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(padded_row),
                rows_per_image: Some(region.size.y),
            },
        };
        let format = texture.format();
        let bgra = matches!(
            format,
            wgpu::TextureFormat::Bgra8UnormSrgb | wgpu::TextureFormat::Bgra8Unorm
        );
        if Self::needs_blit(format) {
            let CaptureBlit { pipeline, binding } =
                blit.unwrap_or_else(|| panic!("a {format:?} texture needs the capture blit"));
            let texture = |label, format, usage| {
                device.create_texture(&wgpu::TextureDescriptor {
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
            let bind_group = binding.bind_group(device, &cut_view, "palantir.capture.region.bg");
            let encoded_view = encoded.create_view(&wgpu::TextureViewDescriptor::default());
            blit_pipeline::draw(&mut encoder, &bind_group, &encoded_view, pipeline);
            encoder.copy_texture_to_buffer(encoded.as_image_copy(), into_buffer, extent);
        } else {
            encoder.copy_texture_to_buffer(at_region, into_buffer, extent);
        }
        queue.submit(iter::once(encoder.finish()));
        let state = Arc::new(AtomicU8::new(MAP_PENDING));
        let mapped = Arc::clone(&state);
        buffer
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |result| {
                let done = if result.is_ok() { MAP_DONE } else { MAP_FAILED };
                mapped.store(done, Ordering::Release);
            });
        Self {
            staging: Staging {
                buffer,
                padded_row,
                state,
            },
            size: region.size,
            bgra,
        }
    }

    /// Whether the copy's map callback has yet to run. A lost device runs it too, with a failure.
    pub(crate) fn is_pending(&self) -> bool {
        self.staging.state.load(Ordering::Acquire) == MAP_PENDING
    }

    /// Block until the copy's map callback has run. A poll on another thread that shares `gpu`
    /// may collect the callback and run it after this thread's wait returns, so the wait repeats
    /// until the callback has run.
    pub(crate) fn wait_landed(&self, gpu: &Gpu) {
        gpu.wait();
        while self.is_pending() {
            thread::yield_now();
            gpu.wait();
        }
    }

    /// The pixels, once the device has finished the copy and a poll ran its map callback.
    ///
    /// # Panics
    ///
    /// While the copy [`is_pending`](Self::is_pending), and when it failed to map, which only a
    /// lost device does.
    pub(crate) fn take(self) -> Image {
        let staging = &self.staging;
        match staging.state.load(Ordering::Acquire) {
            MAP_PENDING => panic!("a capture taken before its copy landed"),
            MAP_DONE => {}
            _ => panic!("a capture's staging buffer failed to map: the device is lost"),
        }
        let row_bytes = (self.size.x * BYTES_PER_PIXEL) as usize;
        let mut pixels = Vec::with_capacity(row_bytes * self.size.y as usize);
        {
            let data = staging
                .buffer
                .slice(..)
                .get_mapped_range()
                .expect("a mapped staging buffer has its range");
            for row in data.chunks_exact(staging.padded_row as usize) {
                pixels.extend_from_slice(&row[..row_bytes]);
            }
        }
        staging.buffer.unmap();
        if self.bgra {
            for texel in pixels.as_chunks_mut::<{ BYTES_PER_PIXEL as usize }>().0 {
                texel.swap(0, 2);
            }
        }
        Image::from_srgba8(self.size, pixels)
            .expect("a copy of a non-empty region holds four bytes per pixel")
    }
}

#[cfg(test)]
pub(crate) mod internals {
    use crate::gpu::surface::capture::{Capture, MAP_DONE, MAP_PENDING, Staging};
    use glam::UVec2;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU8, Ordering};
    use std::thread;
    use std::time::Duration;

    impl Capture {
        /// A capture whose copy never finishes, as one the device is still running.
        pub(crate) fn never_landing(device: &wgpu::Device) -> Self {
            Self::with_state(device, Arc::new(AtomicU8::new(MAP_PENDING)))
        }

        /// A capture whose map callback another thread runs `delay` from now, as one a poll on
        /// that thread collected.
        pub(crate) fn mapped_elsewhere_after(device: &wgpu::Device, delay: Duration) -> Self {
            let state = Arc::new(AtomicU8::new(MAP_PENDING));
            let mapped = Arc::clone(&state);
            thread::spawn(move || {
                thread::sleep(delay);
                mapped.store(MAP_DONE, Ordering::Release);
            });
            Self::with_state(device, state)
        }

        fn with_state(device: &wgpu::Device, state: Arc<AtomicU8>) -> Self {
            let buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("palantir.capture.test"),
                size: u64::from(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT),
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            });
            Self {
                staging: Staging {
                    buffer,
                    padded_row: wgpu::COPY_BYTES_PER_ROW_ALIGNMENT,
                    state,
                },
                size: UVec2::ONE,
                bgra: false,
            }
        }
    }
}
