//! [`BoundSides`] — the group-0 binding over a
//! [`RasterAtlas`](crate::renderer::backend::raster_atlas::RasterAtlas)'s two
//! sides.

use crate::renderer::backend::raster_atlas::content_type::ContentType;
use crate::renderer::backend::raster_atlas::side::Side;
use crate::renderer::backend::texture_binding;

/// The group-0 bind group over `sides`, paired with the `[color, mask]`
/// extents the shader reads as params.
#[derive(Debug)]
pub(super) struct BoundSides {
    pub(super) bind_group: wgpu::BindGroup,
    pub(super) atlas_px: [u32; 2],
}

impl BoundSides {
    /// Group 0: mask at 0, colour at 1, one shared sampler at 2 — the same entry
    /// shapes every other group uses, two textures deep instead of one.
    pub(super) fn layout(device: &wgpu::Device, label: &str) -> wgpu::BindGroupLayout {
        device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some(label),
            entries: &[
                texture_binding::texture_entry(0),
                texture_binding::texture_entry(1),
                texture_binding::sampler_entry(2),
            ],
        })
    }

    /// Nearest on both axes: a quad is drawn at its slot's own pixel dimensions,
    /// so every texel maps 1:1 and filtering could only blur what the rasterizer
    /// already got exactly right.
    pub(super) fn sampler(device: &wgpu::Device, label: &str) -> wgpu::Sampler {
        device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some(label),
            min_filter: wgpu::FilterMode::Nearest,
            mag_filter: wgpu::FilterMode::Nearest,
            mipmap_filter: wgpu::MipmapFilterMode::Nearest,
            ..Default::default()
        })
    }

    /// The group-0 bind group itself, over the two side views and the
    /// shared sampler.
    fn bind_group(
        device: &wgpu::Device,
        layout: &wgpu::BindGroupLayout,
        mask_view: &wgpu::TextureView,
        color_view: &wgpu::TextureView,
        sampler: &wgpu::Sampler,
        label: &str,
    ) -> wgpu::BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some(label),
            layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(mask_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(color_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(sampler),
                },
            ],
        })
    }

    /// Bind `sides` for sampling. One definition rather than one per call
    /// site: `RasterAtlas::new` and `RasterAtlas::grow` both need the
    /// pair, and building it two ways is how the extents and the views they
    /// describe drift apart.
    pub(super) fn new(
        device: &wgpu::Device,
        sides: &[Side; 2],
        bgl: &wgpu::BindGroupLayout,
        sampler: &wgpu::Sampler,
        label: &str,
    ) -> Self {
        let mask = &sides[ContentType::Mask as usize];
        let color = &sides[ContentType::Color as usize];
        Self {
            bind_group: Self::bind_group(device, bgl, &mask.view, &color.view, sampler, label),
            atlas_px: [color.size, mask.size],
        }
    }
}
