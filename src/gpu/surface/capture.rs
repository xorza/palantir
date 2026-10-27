//! [`Capture`]: a region of a painted frame on its way back from the device.

use crate::gpu::surface::backbuffer::Backbuffer;
use crate::primitives::geometry::urect::URect;
use crate::primitives::paint::image::Image;
use glam::UVec2;
use std::iter;
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};

const BYTES_PER_PIXEL: u32 = 4;

const MAP_PENDING: u8 = 0;
const MAP_DONE: u8 = 1;
const MAP_FAILED: u8 = 2;

/// One region of a frame, copied from the backbuffer into a staging buffer that maps without
/// blocking the frame; [`Self::take`] hands the pixels over once the device is done.
#[derive(Debug)]
pub(crate) struct Capture {
    /// `None` for an empty region, which needs no copy.
    staging: Option<Staging>,
    size: UVec2,
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

/// What a finished [`Capture`] holds.
#[derive(Debug)]
pub(crate) struct Captured {
    /// `None` for an empty region.
    pub(crate) image: Option<Image>,
}

impl Capture {
    /// The image a capture always lands in: what an image file holds, whatever the frame's target
    /// format, so a BGRA or float target reads back as every other.
    pub(crate) const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;

    /// Start copying `region` of `backbuffer`, which holds the frame just submitted. The frame is
    /// drawn into a [`Self::FORMAT`] texture with `blit` (built for that format), the region
    /// copied out of it, and the copy mapped once the device finishes.
    pub(crate) fn start(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        blit: &wgpu::RenderPipeline,
        backbuffer: &Backbuffer,
        region: URect,
    ) -> Self {
        if region.size.x == 0 || region.size.y == 0 {
            return Self {
                staging: None,
                size: UVec2::ZERO,
            };
        }
        let full = backbuffer.size();
        debug_assert!(
            region.min.x + region.size.x <= full.x && region.min.y + region.size.y <= full.y,
            "a capture region {region:?} inside the {full} backbuffer"
        );
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("palantir.capture"),
            size: wgpu::Extent3d {
                width: full.x,
                height: full.y,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: Self::FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
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
        backbuffer.draw_onto(&mut encoder, &view, blit);
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d {
                    x: region.min.x,
                    y: region.min.y,
                    z: 0,
                },
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded_row),
                    rows_per_image: Some(region.size.y),
                },
            },
            wgpu::Extent3d {
                width: region.size.x,
                height: region.size.y,
                depth_or_array_layers: 1,
            },
        );
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
            staging: Some(Staging {
                buffer,
                padded_row,
                state,
            }),
            size: region.size,
        }
    }

    /// Whether the copy's map callback has yet to run. A lost device runs it too, with a failure.
    pub(crate) fn is_pending(&self) -> bool {
        self.staging
            .as_ref()
            .is_some_and(|staging| staging.state.load(Ordering::Acquire) == MAP_PENDING)
    }

    /// The pixels, once the device has finished the copy and a poll ran its map callback; `None`
    /// before that.
    ///
    /// # Panics
    ///
    /// When the copy failed to map, which only a lost device does.
    pub(crate) fn take(&self) -> Option<Captured> {
        let Some(staging) = &self.staging else {
            return Some(Captured { image: None });
        };
        match staging.state.load(Ordering::Acquire) {
            MAP_PENDING => return None,
            MAP_DONE => {}
            _ => panic!("a screenshot's staging buffer failed to map: the device is lost"),
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
        let image = Image::from_srgba8(self.size, pixels)
            .expect("a copy of a non-empty region holds four bytes per pixel");
        Some(Captured { image: Some(image) })
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
                staging: Some(Staging {
                    buffer,
                    padded_row: wgpu::COPY_BYTES_PER_ROW_ALIGNMENT,
                    state,
                }),
                size: UVec2::ONE,
            }
        }
    }
}
