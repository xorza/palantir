//! The device and queue Palantir draws with.

use crate::gpu::device::device_requirements::DeviceRequirements;
use crate::gpu::error::UnmetRequirements;
use std::num::NonZeroU32;

/// A graphics device and its queue.
///
/// An application that opened its own wraps the pair here for
/// [`OffscreenHost::builder`](crate::OffscreenHost::builder); a host that
/// used [`RequestedGpu`](crate::RequestedGpu) finds it in
/// [`gpu`](crate::RequestedGpu::gpu). Cloning
/// clones the handles, not the device.
#[derive(Clone, Debug)]
pub struct Gpu {
    pub(crate) device: wgpu::Device,
    pub(crate) queue: wgpu::Queue,
}

impl Gpu {
    /// Draw through a device the caller owns. It must have been requested with
    /// [`DeviceRequirements`]; the host builders check that.
    pub const fn new(device: wgpu::Device, queue: wgpu::Queue) -> Self {
        Self { device, queue }
    }

    pub(crate) fn requirements_met(&self) -> Result<(), UnmetRequirements> {
        DeviceRequirements::met_by(&self.device)
    }

    /// The device's `max_texture_dimension_2d`: the ceiling on registered
    /// images, `GpuView` targets, and the glyph and gradient atlases.
    ///
    /// A zero limit panics rather than joining [`DeviceRequirements::met_by`]'s
    /// `Result`: no adapter reports one.
    pub(crate) fn max_texture_dim(&self) -> NonZeroU32 {
        NonZeroU32::new(self.device.limits().max_texture_dimension_2d)
            .expect("device texture dimension limit is zero")
    }
}

/// Read by tools that check what a frame drew from outside: the visual suite and harnesses.
#[cfg(feature = "internals")]
pub(crate) mod internals {
    use std::iter;
    use std::sync::mpsc;

    use image::RgbaImage;

    use crate::gpu::device::gpu::Gpu;

    const BYTES_PER_PIXEL: u32 = 4;

    impl Gpu {
        /// `texture`'s pixels, copied back from the device and waited for, in RGBA order: a BGRA
        /// texture is swizzled, so every caller reads one channel order. An sRGB texture's bytes are
        /// its encoded values, as an image file holds them.
        ///
        /// # Panics
        ///
        /// For a texture that is not one single-sampled 2D image in 8-bit RGBA or BGRA, or lacks
        /// `COPY_SRC`, and when the device is lost before the copy maps.
        pub fn read_back(&self, texture: &wgpu::Texture) -> RgbaImage {
            let swizzle = match texture.format() {
                wgpu::TextureFormat::Rgba8Unorm | wgpu::TextureFormat::Rgba8UnormSrgb => false,
                wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Bgra8UnormSrgb => true,
                other => panic!("read_back takes an 8-bit RGBA or BGRA texture, got {other:?}"),
            };
            assert!(
                texture.usage().contains(wgpu::TextureUsages::COPY_SRC),
                "read_back needs a texture made with COPY_SRC"
            );
            assert!(
                texture.dimension() == wgpu::TextureDimension::D2
                    && texture.depth_or_array_layers() == 1
                    && texture.sample_count() == 1,
                "read_back copies one single-sampled 2D image"
            );
            let size = texture.size();
            let row_bytes = size.width * BYTES_PER_PIXEL;
            // The copy writes whole aligned rows; the padding is cut below.
            let padded = row_bytes.div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT)
                * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
            let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("palantir.read_back"),
                size: u64::from(padded) * u64::from(size.height),
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            });
            let mut encoder = self
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("palantir.read_back"),
                });
            encoder.copy_texture_to_buffer(
                wgpu::TexelCopyTextureInfo {
                    texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                wgpu::TexelCopyBufferInfo {
                    buffer: &buffer,
                    layout: wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(padded),
                        rows_per_image: Some(size.height),
                    },
                },
                wgpu::Extent3d {
                    width: size.width,
                    height: size.height,
                    depth_or_array_layers: 1,
                },
            );
            self.queue.submit(iter::once(encoder.finish()));

            let slice = buffer.slice(..);
            let (tx, rx) = mpsc::channel();
            slice.map_async(wgpu::MapMode::Read, move |mapped| {
                tx.send(mapped).ok();
            });
            self.device
                .poll(wgpu::PollType::Wait {
                    submission_index: None,
                    timeout: None,
                })
                .expect("the device finishes the read-back copy");
            rx.recv()
                .expect("the map callback runs once the device finishes")
                .expect("the read-back buffer maps");

            let data = slice
                .get_mapped_range()
                .expect("the mapped read-back range");
            let mut out = Vec::with_capacity(row_bytes as usize * size.height as usize);
            for row in data.chunks_exact(padded as usize) {
                out.extend_from_slice(&row[..row_bytes as usize]);
            }
            drop(data);
            buffer.unmap();
            if swizzle {
                for pixel in out.as_chunks_mut::<{ BYTES_PER_PIXEL as usize }>().0 {
                    pixel.swap(0, 2);
                }
            }
            RgbaImage::from_raw(size.width, size.height, out)
                .expect("one RGBA pixel per texel of the texture")
        }
    }

    #[cfg(test)]
    mod tests {
        use std::iter;

        use glam::UVec2;
        use image::Rgba;

        use crate::gpu::surface::render_target::internals::TargetSpec;
        use crate::gpu::test_gpu::{HeadlessTestGpuLease, headless_test_gpu};

        /// A 3 × 2 texture cleared to (0.2, 0.4, 0.6, 1.0) reads back as six pixels of
        /// (51, 102, 153, 255) — 0.2 · 255 = 51, 0.4 · 255 = 102, 0.6 · 255 = 153 — in both
        /// channel orders. Three pixels make a 12-byte row, far below the 256-byte copy alignment,
        /// so the padding is cut from every row.
        #[test]
        fn a_cleared_texture_reads_back_in_rgba_order() {
            let gpu = headless_test_gpu();
            for format in [
                wgpu::TextureFormat::Rgba8Unorm,
                wgpu::TextureFormat::Bgra8Unorm,
            ] {
                let texture = gpu.target_with(TargetSpec {
                    label: "palantir.read_back.test",
                    size: UVec2::new(3, 2),
                    format,
                    usage: HeadlessTestGpuLease::TARGET_USAGES,
                });
                clear(&gpu, &texture);
                let image = gpu.handles().read_back(&texture);
                assert_eq!(image.dimensions(), (3, 2), "{format:?}");
                assert!(
                    image
                        .pixels()
                        .all(|pixel| *pixel == Rgba([51, 102, 153, 255])),
                    "{format:?}: {:?}",
                    image.pixels().collect::<Vec<_>>()
                );
            }
        }

        fn clear(gpu: &HeadlessTestGpuLease, texture: &wgpu::Texture) {
            let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
            let mut encoder = gpu
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
            drop(encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("palantir.read_back.test.clear"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.2,
                            g: 0.4,
                            b: 0.6,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            }));
            gpu.queue.submit(iter::once(encoder.finish()));
        }
    }
}
