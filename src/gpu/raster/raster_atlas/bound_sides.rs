//! [`BoundSides`]: the group-0 binding over a [`RasterAtlas`](crate::gpu::raster::raster_atlas::RasterAtlas)'s two sides.

use crate::gpu::raster::raster_atlas::side::Side;
use crate::gpu::raster::raster_program::RasterProgram;
use crate::primitives::paint::content_type::ContentType;

/// The group-0 binding over an atlas's `[mask, color]` sides, in two tiers: the layout outlives any pair of textures and any atlas, so it belongs to the shared [`RasterProgram`]; the bind group describes current textures and a grow replaces it via [`Self::rebind`].
#[derive(Debug)]
pub(super) struct BoundSides {
    /// A clone of the shared [`RasterProgram`]'s layout.
    layout: wgpu::BindGroupLayout,
    bind_group: wgpu::BindGroup,
    /// Held here so [`Self::rebind`] needs only the device and sides; the debug label tells the two atlases apart in a capture.
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

    /// Re-bind after a grow moved a side's texture. The layout is untouched, so pipelines built against it stay valid.
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
