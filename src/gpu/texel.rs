//! The texel geometry a texture copy and a texture descriptor name.

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
