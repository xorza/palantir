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
        render_target::texture_size(&self.texture)
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
    use crate::gpu::surface::capture::{Capture, CaptureSource};
    use crate::gpu::surface::owned_target::OwnedTarget;
    use crate::primitives::geometry::urect::URect;
    use image::RgbaImage;

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
            let size = self.size();
            let source = CaptureSource {
                texture: &self.texture,
                blit: None,
            };
            let capture = Capture::start(
                &self.gpu.device,
                &self.gpu.queue,
                source,
                URect::new(0, 0, size.x, size.y),
            );
            self.gpu.wait();
            let image = capture
                .take()
                .expect("the read-back copy maps on a device that is not lost");
            RgbaImage::from_raw(
                size.x,
                size.y,
                bytemuck::cast_slice(image.texels()).to_vec(),
            )
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
