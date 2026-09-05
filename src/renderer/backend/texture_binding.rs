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

/// The sampler that fills a [`sampler_entry`] slot — the value twin of
/// that entry builder, the way [`bind_group`] is [`layout`]'s.
///
/// Linear within a mip and nearest between them, clamped on all three
/// axes. Clamping is safe for both users because neither hands the
/// sampler a coordinate outside `0..1`: the gradient shader applies
/// [`Spread`](crate::primitives::brush::gradient::Spread) to `t` before
/// the sample, and the image shader `fract`s its uv under
/// `FLAG_TILED`. One descriptor, so a filter or address change cannot
/// reach one of them and not the other.
///
/// The raster atlases build their own, and should: they sample at
/// exactly one texel per pixel and want `Nearest` throughout.
pub(super) fn sampler(device: &wgpu::Device, label: &'static str) -> wgpu::Sampler {
    device.create_sampler(&wgpu::SamplerDescriptor {
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
/// registered images (`WgpuImageStore::write`), the `GpuView` off-screen
/// target (`AllocatedTarget::new`), and the gradient LUT atlas
/// (`GpuGradientAtlas::new`), so their bindings can't drift.
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
