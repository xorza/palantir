//! The one texture-plus-sampler binding shape every palantir shader that
//! samples a texture declares — its layout entries, the group-0 layout
//! they compose into, and the bind group that fills it.
//!
//! Free functions rather than methods because every type involved is
//! `wgpu`'s; reach them namespace-qualified (`texture_binding::layout`).
//! Split into entries and whole-layout builders because the layouts that
//! need them do not all have the same *arity*: the gradient LUT atlas and
//! the per-image group take one texture, the glyph atlas takes two (mask
//! + colour), so no single builder covers them.

/// One fragment-visible, filterable 2D float texture binding — the only
/// texture shape any palantir shader declares.
///
/// A named entry rather than an inline literal because the layouts that
/// need it do not all have the same *arity*: the gradient LUT atlas and
/// the per-image group take one ([`layout`]), the glyph
/// atlas takes two (mask + colour), so no single layout builder covers
/// them. The entry is the largest piece they can actually share, and
/// sharing it is what keeps a `filterable` or `view_dimension` change
/// from reaching some groups and not others.
pub(super) fn texture_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
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

/// The filtering sampler that pairs with [`texture_entry`].
/// Split out for the same reason: it trails a different number of
/// texture bindings in each layout.
pub(super) fn sampler_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
        count: None,
    }
}

/// Build a group-0 bind-group layout pairing a filterable 2D float
/// texture at binding 0 with a filtering sampler at binding 1, both
/// fragment-visible. The shape shared by the gradient LUT atlas
/// (`GpuGradientAtlas`) and the per-image bind group (`ImagePipeline`).
pub(super) fn layout(device: &wgpu::Device, label: &'static str) -> wgpu::BindGroupLayout {
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some(label),
        entries: &[texture_entry(0), sampler_entry(1)],
    })
}

/// Build a bind group pairing a texture view at binding 0 with a sampler
/// at binding 1 against a [`layout`]-shaped layout — the
/// value twin of that layout builder. One construction site for the
/// CPU-image upload (`ImagePipeline::upload`), the `GpuView` off-screen
/// target (`image_pipeline::render_target::make_target`), and the
/// gradient LUT atlas (`GpuGradientAtlas::new`), so their bindings
/// can't drift.
pub(super) fn bind_group(
    device: &wgpu::Device,
    bgl: &wgpu::BindGroupLayout,
    sampler: &wgpu::Sampler,
    view: &wgpu::TextureView,
    label: &str,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some(label),
        layout: bgl,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(sampler),
            },
        ],
    })
}
