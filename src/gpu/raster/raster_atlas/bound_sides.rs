//! [`BoundSides`]: the group-0 binding over a [`RasterAtlas`](crate::gpu::raster::raster_atlas::RasterAtlas)'s two sides.

use crate::gpu::device::gpu::Gpu;
use crate::gpu::pipeline::raster_pipeline::RasterPipeline;
use crate::gpu::raster::raster_atlas::side::Side;
use crate::primitives::paint::content_type::ContentType;

/// The group-0 binding over an atlas's `[mask, color]` sides. The layout outlives any pair of
/// textures and any atlas, so it is a clone of the shared [`RasterPipeline`]'s; the bind group
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
    pub(super) fn new(gpu: &Gpu, program: &RasterPipeline, stem: &str, sides: &[Side; 2]) -> Self {
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

#[cfg(feature = "internals")]
pub(crate) mod internals {
    use crate::gpu::device::gpu::Gpu;
    use crate::gpu::raster::raster_atlas::bound_sides::BoundSides;
    use crate::gpu::raster::raster_atlas::side::Side;
    use crate::primitives::paint::content_type::ContentType;

    impl BoundSides {
        /// Bind `sides` against `layout`, which takes `pages` mask textures at bindings
        /// `0..pages` and as many color textures after them: each side's one texture in every
        /// slot of its kind. For a bench of a shader that picks among pages; a grow afterwards
        /// binds two textures against `layout` and fails.
        pub(crate) fn bind_pages(
            &mut self,
            gpu: &Gpu,
            layout: &wgpu::BindGroupLayout,
            pages: u32,
            sides: &[Side; 2],
        ) {
            let entries: Vec<wgpu::BindGroupEntry<'_>> = [ContentType::Mask, ContentType::Color]
                .into_iter()
                .enumerate()
                .flat_map(|(kind, content)| {
                    let view = sides[content as usize].atlas.view();
                    (0..pages).map(move |page| wgpu::BindGroupEntry {
                        binding: kind as u32 * pages + page,
                        resource: wgpu::BindingResource::TextureView(view),
                    })
                })
                .collect();
            self.layout = layout.clone();
            self.bind_group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some(&self.label),
                layout,
                entries: &entries,
            });
        }
    }
}
