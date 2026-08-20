//! [`BoundSides`] — everything group 0 needs to sample a
//! [`RasterAtlas`](crate::renderer::backend::raster_atlas::RasterAtlas)'s two
//! sides.

use crate::renderer::backend::raster_atlas::content_type::ContentType;
use crate::renderer::backend::raster_atlas::side::Side;
use crate::renderer::backend::texture_binding;

/// The group-0 binding over an atlas's `[mask, color]` sides.
///
/// Two tiers, and the split is the point. [`Self::layout`] and the sampler
/// are properties of the *shape* of a group-0 binding, so they outlive any
/// one pair of textures — pipelines are created against that layout and
/// would have to be rebuilt if it were replaced. The bind group and the
/// extents describe the textures that exist right now, and a grow replaces
/// both.
///
/// Every field is private and there is no field-wise setter: the only way
/// to move this forward is [`Self::rebind`], which rebuilds the bind group
/// and the extents from one `sides` in one statement. That is what keeps
/// the extents describing the views they are bound beside — building the
/// two separately is how they drift apart.
#[derive(Debug)]
pub(super) struct BoundSides {
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    bind_group: wgpu::BindGroup,
    /// `[color, mask]` — the order the shader reads them as params, which
    /// is the reverse of the `[mask, color]` binding order above it.
    atlas_px: [u32; 2],
    /// Held here rather than passed in, so [`Self::rebind`] needs nothing
    /// but the device and the sides — a caller that has to supply the
    /// label is a caller that can supply a different one, and the debug
    /// label is how a capture tells the two atlases apart.
    label: String,
}

impl BoundSides {
    /// Bind `sides` for sampling, building the layout and sampler the
    /// binding will keep for the rest of its life.
    pub(super) fn new(device: &wgpu::Device, sides: &[Side; 2], stem: &str) -> Self {
        let layout = Self::create_layout(device, &format!("{stem} atlas layout"));
        let sampler = Self::create_sampler(device, &format!("{stem} sampler"));
        let label = format!("{stem} atlas bg");
        let bind_group = Self::create_bind_group(device, &layout, sides, &sampler, &label);
        Self {
            layout,
            sampler,
            bind_group,
            atlas_px: Self::extents(sides),
            label,
        }
    }

    /// Re-bind after a grow moved one side's texture, from the `sides` the
    /// grow left behind.
    ///
    /// Both halves in one statement, so no caller can observe extents that
    /// describe a texture the bind group no longer points at. The layout
    /// and sampler are deliberately untouched — every pipeline built
    /// against [`Self::layout`] stays valid across any number of grows.
    pub(super) fn rebind(&mut self, device: &wgpu::Device, sides: &[Side; 2]) {
        self.bind_group =
            Self::create_bind_group(device, &self.layout, sides, &self.sampler, &self.label);
        self.atlas_px = Self::extents(sides);
    }

    /// The layout every bind group this ever builds is built against —
    /// created once and never replaced, which is what lets a pipeline hold
    /// it across any number of [`Self::rebind`]s.
    pub(super) fn layout(&self) -> &wgpu::BindGroupLayout {
        &self.layout
    }

    pub(super) fn bind_group(&self) -> &wgpu::BindGroup {
        &self.bind_group
    }

    /// The extents as of the last bind — see the field for the lane order.
    pub(super) fn atlas_px(&self) -> [u32; 2] {
        self.atlas_px
    }

    /// The one place the param order is spelled, so [`Self::new`] and
    /// [`Self::rebind`] cannot disagree about which side comes first.
    fn extents(sides: &[Side; 2]) -> [u32; 2] {
        [
            sides[ContentType::Color as usize].size,
            sides[ContentType::Mask as usize].size,
        ]
    }

    /// Group 0: mask at 0, colour at 1, one shared sampler at 2 — the
    /// same entry shapes every other group uses, two textures deep
    /// instead of one.
    fn create_layout(device: &wgpu::Device, label: &str) -> wgpu::BindGroupLayout {
        device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some(label),
            entries: &[
                texture_binding::texture_entry(0),
                texture_binding::texture_entry(1),
                texture_binding::sampler_entry(2),
            ],
        })
    }

    /// Nearest on both axes: a quad is drawn at its slot's own pixel
    /// dimensions, so every texel maps 1:1 and filtering could only blur
    /// what the rasterizer already got exactly right.
    fn create_sampler(device: &wgpu::Device, label: &str) -> wgpu::Sampler {
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
    fn create_bind_group(
        device: &wgpu::Device,
        layout: &wgpu::BindGroupLayout,
        sides: &[Side; 2],
        sampler: &wgpu::Sampler,
        label: &str,
    ) -> wgpu::BindGroup {
        let mask = &sides[ContentType::Mask as usize];
        let color = &sides[ContentType::Color as usize];
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some(label),
            layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&mask.view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&color.view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(sampler),
                },
            ],
        })
    }
}
