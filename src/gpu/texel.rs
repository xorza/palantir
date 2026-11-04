//! The texel geometry a texture copy and a texture descriptor name.

use crate::gpu::device::gpu::Gpu;
use crate::gpu::surface::owned_target::TargetSpec;
use glam::UVec2;

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
