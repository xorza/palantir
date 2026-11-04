//! [`BoundSides`]: the group-0 binding over a [`RasterAtlas`](crate::gpu::raster::raster_atlas::RasterAtlas)'s two sides.

use crate::gpu::device::gpu::Gpu;
use crate::gpu::pipeline::raster_pipeline::RasterProgram;
use crate::gpu::raster::raster_atlas::side::Side;
use crate::primitives::paint::content_type::ContentType;

/// The group-0 binding over an atlas's `[mask, color]` sides. The layout outlives any pair of
/// textures and any atlas, so it is a clone of the shared [`RasterProgram`]'s; the bind group
/// describes the current textures, and a grow replaces it through [`Self::rebind`].
#[derive(Debug)]
pub(super) struct BoundSides {
    layout: wgpu::BindGroupLayout,
    /// Tells the two atlases apart in a capture.
    label: String,
    bind_group: wgpu::BindGroup,
}

impl BoundSides {
    /// Bind `sides` against `program`'s layout, labeled after `stem`.
    pub(super) fn new(gpu: &Gpu, program: &RasterProgram, stem: &str, sides: &[Side; 2]) -> Self {
        let layout = program.layout().clone();
        let label = format!("{stem} atlas bg");
        let bind_group = bind(gpu, &layout, &label, sides);
        Self {
            layout,
            label,
            bind_group,
        }
    }

    /// Re-bind after a grow moved a side's texture. The layout is untouched, so pipelines built against it stay valid.
    pub(super) fn rebind(&mut self, gpu: &Gpu, sides: &[Side; 2]) {
        self.bind_group = bind(gpu, &self.layout, &self.label, sides);
    }

    pub(super) const fn bind_group(&self) -> &wgpu::BindGroup {
        &self.bind_group
    }
}

/// The group-0 bind group over the two side views.
fn bind(
    gpu: &Gpu,
    layout: &wgpu::BindGroupLayout,
    label: &str,
    sides: &[Side; 2],
) -> wgpu::BindGroup {
    let mask = &sides[ContentType::Mask as usize];
    let color = &sides[ContentType::Color as usize];
    gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some(label),
        layout,
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
