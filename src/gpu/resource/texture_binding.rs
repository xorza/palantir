//! [`TextureBinding`] — the one texture-plus-sampler group every palantir
//! shader that samples a texture binds.

/// The group-0 layout every sampled texture binds through, and the sampler
/// it pairs with: the gradient LUT atlas, the registered images, the
/// `GpuView` targets, and the backbuffer when it is drawn onto a target
/// that takes no copy.
///
/// Built once by the backend. `Clone` hands out `wgpu`'s own
/// reference-counted handles, so every holder shares one layout and one
/// sampler, and every pipeline that samples one of these textures composes
/// over that same layout — a group built for one binds in any of them.
#[derive(Clone, Debug)]
pub(crate) struct TextureBinding {
    layout: wgpu::BindGroupLayout,
    /// Linear within a mip and nearest between them, clamped on all three
    /// axes. Clamping is safe for every user because none hands the sampler
    /// a coordinate outside `0..1`: the gradient shader applies
    /// [`Spread`](crate::primitives::paint::brush::gradient::Spread) to `t`
    /// before the sample, and the image shader `fract`s its uv under
    /// `FLAG_TILED`. Nearest image filtering is a shader-side snap to the
    /// texel centre, so every filter combination rides this one sampler.
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

    /// One fragment-visible, filterable 2D float texture entry — the only
    /// texture shape any palantir shader declares.
    ///
    /// Shared beyond this layout because the raster atlases bind two such
    /// textures and no sampler, so their layout is their own; sharing the
    /// entry is what keeps a `filterable` or `view_dimension` change from
    /// reaching one layout and not the other.
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

    /// The group that binds `view` through this layout and sampler.
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
