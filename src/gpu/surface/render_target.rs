//! The texture a frame renders into, and its format.

use glam::UVec2;

/// A texture Palantir renders a frame into; borrowed.
#[derive(Clone, Copy, Debug)]
pub struct RenderTarget<'a> {
    texture: &'a wgpu::Texture,
}

impl<'a> RenderTarget<'a> {
    /// Render into `texture`; the format must be sRGB or float, usage must include `RENDER_ATTACHMENT`.
    ///
    /// # Panics
    ///
    /// Panics on a non-sRGB/float format or usage lacking `RENDER_ATTACHMENT`.
    #[track_caller]
    pub fn new(texture: &'a wgpu::Texture) -> Self {
        assert!(
            texture
                .usage()
                .contains(wgpu::TextureUsages::RENDER_ATTACHMENT),
            "a render target must allow RENDER_ATTACHMENT",
        );
        TargetFormat::new(texture.format());
        Self { texture }
    }

    pub(crate) fn size(self) -> UVec2 {
        let size = self.texture.size();
        UVec2::new(size.width, size.height)
    }

    pub(crate) fn format(self) -> TargetFormat {
        TargetFormat(self.texture.format())
    }

    /// Whether a frame can be copied onto this target (false for a GLES swapchain image).
    pub(crate) fn takes_copy(self) -> bool {
        self.texture.usage().contains(wgpu::TextureUsages::COPY_DST)
    }

    pub(crate) const fn texture(self) -> &'a wgpu::Texture {
        self.texture
    }
}

/// The texel format of a [`RenderTarget`]; opaque, only compared and keyed on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TargetFormat(wgpu::TextureFormat);

impl TargetFormat {
    /// Name a format directly, under [`RenderTarget::new`]'s rule.
    ///
    /// # Panics
    ///
    /// Panics when `format` is neither sRGB nor float.
    #[track_caller]
    pub fn new(format: wgpu::TextureFormat) -> Self {
        assert!(
            Self::encodes_linear(format),
            "a render target's format must be sRGB or float",
        );
        Self(format)
    }

    /// True if a target in `format` encodes linear light (unorm renders too dark).
    fn encodes_linear(format: wgpu::TextureFormat) -> bool {
        format.is_srgb()
            || matches!(
                format,
                wgpu::TextureFormat::Rgba16Float
                    | wgpu::TextureFormat::Rgba32Float
                    | wgpu::TextureFormat::Rg11b10Ufloat
            )
    }

    pub(crate) const fn get(self) -> wgpu::TextureFormat {
        self.0
    }
}

/// A render-target extent; depth is always one.
pub(crate) const fn extent(size: UVec2) -> wgpu::Extent3d {
    wgpu::Extent3d {
        width: size.x,
        height: size.y,
        depth_or_array_layers: 1,
    }
}

#[cfg(any(test, feature = "internals"))]
pub(crate) mod internals {
    use crate::gpu::surface::render_target;
    use glam::UVec2;

    /// A single-sample 2D render target of `size`.
    pub(crate) fn texture(
        device: &wgpu::Device,
        label: &str,
        size: UVec2,
        format: wgpu::TextureFormat,
        usage: wgpu::TextureUsages,
    ) -> wgpu::Texture {
        device.create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: render_target::extent(size),
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage,
            view_formats: &[],
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::gpu::surface::render_target::internals::texture;
    use crate::gpu::surface::render_target::{RenderTarget, TargetFormat};
    use crate::gpu::test_gpu::headless_test_gpu;
    use crate::internals::panic_probe;
    use glam::UVec2;

    /// Panics when the format is not sRGB/float or usage lacks a render pass.
    #[test]
    fn targets_check_their_format_and_usage() {
        use wgpu::{TextureFormat as F, TextureUsages as U};

        for format in [F::Rgba8UnormSrgb, F::Bgra8UnormSrgb, F::Rgba16Float] {
            let _ = TargetFormat::new(format);
        }
        for format in [F::Rgba8Unorm, F::Bgra8Unorm, F::R8Unorm] {
            panic_probe::assert_panics_with("format must be sRGB or float", || {
                TargetFormat::new(format)
            });
        }

        let gpu = headless_test_gpu();
        let made =
            |format, usage| texture(&gpu.device, "target-check", UVec2::splat(4), format, usage);
        let good = made(F::Rgba8UnormSrgb, U::RENDER_ATTACHMENT);
        assert_eq!(RenderTarget::new(&good).size(), UVec2::splat(4));
        let linear = made(F::Rgba8Unorm, U::RENDER_ATTACHMENT);
        panic_probe::assert_panics_with("format must be sRGB or float", || {
            RenderTarget::new(&linear)
        });
        let sampled = made(F::Rgba8UnormSrgb, U::TEXTURE_BINDING);
        panic_probe::assert_panics_with("must allow RENDER_ATTACHMENT", || {
            RenderTarget::new(&sampled)
        });
    }
}
