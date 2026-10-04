//! [`BoundSides`] — everything group 0 needs to read a
//! [`RasterAtlas`](crate::gpu::raster::raster_atlas::RasterAtlas)'s two
//! sides.

use crate::gpu::raster::raster_atlas::side::Side;
use crate::gpu::raster::raster_program::RasterProgram;
use crate::primitives::paint::content_type::ContentType;

/// The group-0 binding over an atlas's `[mask, color]` sides.
///
/// Two tiers, and the split is the point. The layout is a property of the
/// *shape* of a group-0 binding, so it outlives any one pair of textures —
/// and outlives any one *atlas*, which is why it belongs to the shared
/// [`RasterProgram`] rather than here. The bind group describes the
/// textures that exist right now, and a grow replaces it through
/// [`Self::rebind`].
#[derive(Debug)]
pub(super) struct BoundSides {
    /// A clone of the shared [`RasterProgram`]'s, so a rebind needs
    /// nothing but the device and the sides.
    layout: wgpu::BindGroupLayout,
    bind_group: wgpu::BindGroup,
    /// Held here rather than passed in, so [`Self::rebind`] needs nothing
    /// but the device and the sides — a caller that has to supply the
    /// label is a caller that can supply a different one, and the debug
    /// label is how a capture tells the two atlases apart.
    label: String,
}

impl BoundSides {
    /// Bind `sides` against the shared program's layout.
    pub(super) fn new(
        device: &wgpu::Device,
        program: &RasterProgram,
        sides: &[Side; 2],
        stem: &str,
    ) -> Self {
        let layout = program.layout().clone();
        let label = format!("{stem} atlas bg");
        let bind_group = Self::create_bind_group(device, &layout, sides, &label);
        Self {
            layout,
            bind_group,
            label,
        }
    }

    /// Re-bind after a grow moved one side's texture, from the `sides` the
    /// grow left behind. The layout is deliberately untouched — every
    /// pipeline built against it stays valid across any number of grows.
    pub(super) fn rebind(&mut self, device: &wgpu::Device, sides: &[Side; 2]) {
        self.bind_group = Self::create_bind_group(device, &self.layout, sides, &self.label);
    }

    pub(super) const fn bind_group(&self) -> &wgpu::BindGroup {
        &self.bind_group
    }

    /// The group-0 bind group itself, over the two side views.
    fn create_bind_group(
        device: &wgpu::Device,
        layout: &wgpu::BindGroupLayout,
        sides: &[Side; 2],
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
            ],
        })
    }
}
