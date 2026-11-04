//! [`TextureBinding`]: the texture-plus-sampler group every texture-sampling shader binds.

use crate::gpu::device::gpu::Gpu;

/// How a shader reads a texture: through a filtering sampler, or texel by texel, which a format
/// that no filter reads, as the cutouts' `R32Float`, needs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Filtering {
    Filtered,
    Loaded,
}

/// The group-0 layout every sampled texture binds through, with its sampler: gradient LUT atlas, registered images, `GpuView` targets, and the backbuffer when drawn onto a no-copy target.
///
/// Built once per device, by the host as it opens the backend, which hands a copy to the image store. `Clone` shares `wgpu`'s ref-counted handles, so every pipeline composes over the same layout and a group built for one binds in any.
#[derive(Clone, Debug)]
pub(crate) struct TextureBinding {
    layout: wgpu::BindGroupLayout,
    /// Linear within a mip, nearest between, clamped on all axes. Safe because no user passes a coordinate outside `0..1`: the gradient shader applies [`Spread`](crate::primitives::paint::brush::gradient::Spread) to `t` first and the image shader `fract`s uv under `FLAG_TILED`. Nearest image filtering is a shader-side snap, so all filters ride this sampler.
    sampler: wgpu::Sampler,
}

impl TextureBinding {
    pub(crate) fn new(gpu: &Gpu) -> Self {
        let layout = gpu
            .device
            .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("palantir.texture.bgl"),
                entries: &[texture_entry(0, Filtering::Filtered), sampler_entry(1)],
            });
        let sampler = sampler(gpu, "palantir.texture.sampler");
        Self { layout, sampler }
    }

    pub(crate) const fn layout(&self) -> &wgpu::BindGroupLayout {
        &self.layout
    }

    /// The group binding `view` through this layout and sampler.
    pub(crate) fn bind_group(
        &self,
        gpu: &Gpu,
        view: &wgpu::TextureView,
        label: &str,
    ) -> wgpu::BindGroup {
        gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
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

/// The backend's one sampler shape: linear within a mip, nearest between, clamped on every
/// axis. Every sampling shader takes it, so its description lives here alone.
pub(crate) fn sampler(gpu: &Gpu, label: &str) -> wgpu::Sampler {
    gpu.device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some(label),
        address_mode_u: wgpu::AddressMode::ClampToEdge,
        address_mode_v: wgpu::AddressMode::ClampToEdge,
        address_mode_w: wgpu::AddressMode::ClampToEdge,
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        mipmap_filter: wgpu::MipmapFilterMode::Nearest,
        ..Default::default()
    })
}

/// One fragment-visible 2D float texture entry, the only texture shape any shader declares,
/// read through a sampler or loaded texel by texel as `filtering` says, so every layout
/// spells it alike.
pub(crate) const fn texture_entry(
    binding: u32,
    filtering: Filtering,
) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float {
                filterable: matches!(filtering, Filtering::Filtered),
            },
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
        },
        count: None,
    }
}

/// A fragment-visible filtering sampler entry, beside a [`texture_entry`].
pub(crate) const fn sampler_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
        count: None,
    }
}
