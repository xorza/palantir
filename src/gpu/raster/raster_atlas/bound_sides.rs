//! [`BoundSides`]: the group-0 binding over a [`RasterAtlas`](crate::gpu::raster::raster_atlas::RasterAtlas)'s two sides.

use crate::gpu::raster::raster_atlas::side::Side;
use crate::gpu::raster::raster_program::RasterProgram;
use crate::primitives::paint::content_type::ContentType;

/// The group-0 binding over an atlas's `[mask, color]` sides, in two tiers: the layout outlives any pair of textures and any atlas, so it belongs to the shared [`RasterProgram`]; the bind group describes current textures and a grow replaces it via [`Self::rebind`].
#[derive(Debug)]
pub(super) struct BoundSides {
    layout: SidesLayout,
    bind_group: wgpu::BindGroup,
}

/// The shared program's layout and the label every group this atlas binds on it carries.
#[derive(Debug)]
pub(super) struct SidesLayout {
    /// A clone of the shared [`RasterProgram`]'s layout.
    layout: wgpu::BindGroupLayout,
    /// Tells the two atlases apart in a capture.
    label: String,
}

impl SidesLayout {
    pub(super) fn new(program: &RasterProgram, stem: &str) -> Self {
        Self {
            layout: program.layout().clone(),
            label: format!("{stem} atlas bg"),
        }
    }

    /// The group-0 bind group itself, over the two side views.
    fn bind(&self, device: &wgpu::Device, sides: &[Side; 2]) -> wgpu::BindGroup {
        let mask = &sides[ContentType::Mask as usize];
        let color = &sides[ContentType::Color as usize];
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some(&self.label),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(mask.atlas.view()),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(color.atlas.view()),
                },
            ],
        })
    }
}

impl BoundSides {
    /// Bind `sides` against `layout`.
    pub(super) fn new(device: &wgpu::Device, layout: SidesLayout, sides: &[Side; 2]) -> Self {
        let bind_group = layout.bind(device, sides);
        Self { layout, bind_group }
    }

    /// Re-bind after a grow moved a side's texture. The layout is untouched, so pipelines built against it stay valid.
    pub(super) fn rebind(&mut self, device: &wgpu::Device, sides: &[Side; 2]) {
        self.bind_group = self.layout.bind(device, sides);
    }

    pub(super) const fn bind_group(&self) -> &wgpu::BindGroup {
        &self.bind_group
    }
}
