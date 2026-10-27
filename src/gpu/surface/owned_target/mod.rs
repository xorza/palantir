//! [`OwnedTarget`]: a render target texture made on a [`Gpu`] and kept with it, for the hosts,
//! harnesses and benches that own the texture they draw into.

use crate::gpu::device::gpu::Gpu;
use crate::gpu::surface::render_target::{self, RenderTarget, TargetFormat};
use glam::UVec2;

/// What an [`OwnedTarget`] is made as: `label` names it in RenderDoc and validation errors.
#[derive(Clone, Copy, Debug)]
pub struct TargetSpec<'a> {
    /// The debug name.
    pub label: &'a str,
    /// Physical pixels.
    pub size: UVec2,
    /// The texel format.
    pub format: wgpu::TextureFormat,
    /// What the texture may be used for.
    pub usage: wgpu::TextureUsages,
}

/// A single-sample 2D texture and the device it lives on.
#[derive(Debug)]
pub struct OwnedTarget {
    texture: wgpu::Texture,
    gpu: Gpu,
}

impl OwnedTarget {
    /// What a window's target allows: draw into it, copy either way.
    pub const USAGES: wgpu::TextureUsages = wgpu::TextureUsages::RENDER_ATTACHMENT
        .union(wgpu::TextureUsages::COPY_DST)
        .union(wgpu::TextureUsages::COPY_SRC);

    /// A texture on `gpu` as `spec` describes it.
    ///
    /// # Panics
    ///
    /// Panics unless each side of `spec.size` is in `1..=` the device's largest 2D texture, as
    /// wgpu would panic deep inside on a bad extent.
    pub fn new(gpu: &Gpu, spec: TargetSpec<'_>) -> Self {
        let TargetSpec {
            label,
            size,
            format,
            usage,
        } = spec;
        let max = gpu.max_texture_dim().get();
        assert!(
            size.min_element() > 0 && size.max_element() <= max,
            "a target's size is 1..={max} physical pixels per side, got {size}",
        );
        let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: render_target::extent(size),
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage,
            view_formats: &[],
        });
        Self {
            texture,
            gpu: gpu.clone(),
        }
    }

    /// A window's target of `size` in `format`, with [`Self::USAGES`].
    ///
    /// # Panics
    ///
    /// As [`Self::new`].
    pub fn window(gpu: &Gpu, label: &str, size: UVec2, format: TargetFormat) -> Self {
        Self::new(
            gpu,
            TargetSpec {
                label,
                size,
                format: format.get(),
                usage: Self::USAGES,
            },
        )
    }

    /// The target a host renders into.
    ///
    /// # Panics
    ///
    /// As [`RenderTarget::new`], for a texture that is not a render target.
    pub fn as_target(&self) -> RenderTarget<'_> {
        RenderTarget::new(&self.texture)
    }

    /// Physical pixels.
    pub fn size(&self) -> UVec2 {
        let size = self.texture.size();
        UVec2::new(size.width, size.height)
    }

    /// A target like this one, at `size`.
    ///
    /// # Panics
    ///
    /// As [`Self::new`].
    pub(crate) fn resized(&self, label: &str, size: UVec2) -> Self {
        Self::new(
            &self.gpu,
            TargetSpec {
                label,
                size,
                format: self.texture.format(),
                usage: self.texture.usage(),
            },
        )
    }
}

#[cfg(feature = "internals")]
pub(crate) mod internals {
    use crate::gpu::device::gpu::Gpu;
    use crate::gpu::surface::owned_target::OwnedTarget;
    use crate::gpu::surface::render_target;
    use image::RgbaImage;
    use std::iter;
    use std::sync::mpsc;

    const BYTES_PER_PIXEL: u32 = 4;

    impl OwnedTarget {
        /// The pixels, copied back from the device and waited for, in RGBA order: a BGRA texture is
        /// swizzled, so every caller reads one channel order. An sRGB texture's bytes are its encoded
        /// values, as an image file holds them.
        ///
        /// # Panics
        ///
        /// For a texture not in 8-bit RGBA or BGRA, or made without `COPY_SRC`, and when the device
        /// is lost before the copy maps.
        pub fn read_back(&self) -> RgbaImage {
            let texture = &self.texture;
            let swizzle = match texture.format() {
                wgpu::TextureFormat::Rgba8Unorm | wgpu::TextureFormat::Rgba8UnormSrgb => false,
                wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Bgra8UnormSrgb => true,
                other => panic!("read_back takes an 8-bit RGBA or BGRA texture, got {other:?}"),
            };
            assert!(
                texture.usage().contains(wgpu::TextureUsages::COPY_SRC),
                "read_back needs a texture made with COPY_SRC"
            );
            let size = self.size();
            let row_bytes = size.x * BYTES_PER_PIXEL;
            // The copy writes whole aligned rows; the padding is cut below.
            let padded = row_bytes.div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT)
                * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
            let Gpu { device, queue } = &self.gpu;
            let buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("palantir.read_back"),
                size: u64::from(padded) * u64::from(size.y),
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            });
            let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
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
                        rows_per_image: Some(size.y),
                    },
                },
                render_target::extent(size),
            );
            queue.submit(iter::once(encoder.finish()));

            let slice = buffer.slice(..);
            let (tx, rx) = mpsc::channel();
            slice.map_async(wgpu::MapMode::Read, move |mapped| {
                tx.send(mapped).ok();
            });
            self.gpu.wait();
            rx.recv()
                .expect("the map callback runs once the device finishes")
                .expect("the read-back buffer maps");

            let data = slice
                .get_mapped_range()
                .expect("the mapped read-back range");
            let mut out = Vec::with_capacity(row_bytes as usize * size.y as usize);
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
            RgbaImage::from_raw(size.x, size.y, out)
                .expect("one RGBA pixel per texel of the texture")
        }

        pub(crate) fn view(&self) -> wgpu::TextureView {
            self.texture
                .create_view(&wgpu::TextureViewDescriptor::default())
        }

        #[cfg(test)]
        pub(crate) const fn texture(&self) -> &wgpu::Texture {
            &self.texture
        }
    }
}

#[cfg(test)]
mod tests;
