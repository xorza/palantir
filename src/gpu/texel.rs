//! The texel geometry a texture copy and a texture descriptor name, and a target's texel format.

use crate::gpu::device::gpu::Gpu;
use glam::UVec2;

/// What a texture is made as, an [`OwnedTarget`](crate::gpu::surface::owned_target::OwnedTarget)
/// and each texture the backend makes alike: `label` names it in RenderDoc and validation errors.
#[derive(Clone, Copy, Debug)]
#[cfg_attr(
    not(feature = "internals"),
    expect(
        unreachable_pub,
        reason = "`pub` for the `internals` re-export, which a build without it lacks"
    )
)]
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

/// The texel format of a [`RenderTarget`](crate::RenderTarget); opaque, only compared and keyed
/// on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TargetFormat(wgpu::TextureFormat);

impl TargetFormat {
    /// 8-bit sRGB RGBA, what an image file holds: the format of the crate's own window targets.
    pub(crate) const RGBA8_SRGB: Self = Self(wgpu::TextureFormat::Rgba8UnormSrgb);

    /// 16-bit float RGBA, which keeps linear values past 8 bits.
    pub(crate) const RGBA16_FLOAT: Self = Self(wgpu::TextureFormat::Rgba16Float);

    /// Name a format directly, under [`RenderTarget::new`](crate::RenderTarget::new)'s rule.
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

/// A texture's first-layer size, in texels.
pub(crate) fn size(texture: &wgpu::Texture) -> UVec2 {
    UVec2::new(texture.width(), texture.height())
}

/// A 2D extent of `size` texels; depth is always one.
pub(crate) const fn extent(size: UVec2) -> wgpu::Extent3d {
    wgpu::Extent3d {
        width: size.x,
        height: size.y,
        depth_or_array_layers: 1,
    }
}

/// The texel at `at` of a texture's first layer, as a copy names it.
pub(crate) const fn origin(at: UVec2) -> wgpu::Origin3d {
    wgpu::Origin3d {
        x: at.x,
        y: at.y,
        z: 0,
    }
}

/// A single-sample 2D texture of one mip level, as `spec` describes it: the shape every texture
/// of the backend but the blur's mip chain takes.
pub(crate) fn texture_2d(gpu: &Gpu, spec: TargetSpec<'_>) -> wgpu::Texture {
    let TargetSpec {
        label,
        size,
        format,
        usage,
    } = spec;
    gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: extent(size),
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage,
        view_formats: &[],
    })
}
