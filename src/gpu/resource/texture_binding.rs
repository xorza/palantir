//! [`TextureBinding`]: the texture-plus-sampler group every texture-sampling shader binds.

/// The group-0 layout every sampled texture binds through, with its sampler: gradient LUT atlas, registered images, `GpuView` targets, and the backbuffer when drawn onto a no-copy target.
///
/// Built once by the backend. `Clone` shares `wgpu`'s ref-counted handles, so every pipeline composes over the same layout and a group built for one binds in any.
#[derive(Clone, Debug)]
pub(crate) struct TextureBinding {
    layout: wgpu::BindGroupLayout,
    /// Linear within a mip, nearest between, clamped on all axes. Safe because no user passes a coordinate outside `0..1`: the gradient shader applies [`Spread`](crate::primitives::paint::brush::gradient::Spread) to `t` first and the image shader `fract`s uv under `FLAG_TILED`. Nearest image filtering is a shader-side snap, so all filters ride this sampler.
    sampler: wgpu::Sampler,
}

impl TextureBinding {
    pub(crate) fn new(device: &wgpu::Device) -> Self {
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("palantir.texture.bgl"),
            entries: &[
                Self::texture_entry(0),
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("palantir.texture.sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Nearest,
            ..Default::default()
        });
        Self { layout, sampler }
    }

    /// One fragment-visible, filterable 2D float texture entry, the only texture shape any shader declares. Shared because the raster atlases (two textures, no sampler) have their own layout, and sharing the entry keeps `filterable` / `view_dimension` changes in step.
    pub(crate) const fn texture_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
        wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        }
    }

    pub(crate) const fn layout(&self) -> &wgpu::BindGroupLayout {
        &self.layout
    }

    /// The group binding `view` through this layout and sampler.
    pub(crate) fn bind_group(
        &self,
        device: &wgpu::Device,
        view: &wgpu::TextureView,
        label: &str,
    ) -> wgpu::BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some(label),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
            ],
        })
    }
}
