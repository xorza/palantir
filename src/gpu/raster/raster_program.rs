//! The GPU program every raster quad draws through, whichever tenant rasterized it: one shader module and one
//! group-0 layout, so one pipeline pair per swapchain format. Each tenant still owns its atlas; a shared
//! layout shares the shape of a binding, not the space behind it. See [`RasterPass`] for the CPU half.
//!
//! [`RasterPass`]: crate::gpu::raster::raster_pass::RasterPass

use crate::gpu::pipeline::pipeline_recipe::PipelineRecipe;
use crate::gpu::pipeline::shader_body::ShaderBody;
use crate::gpu::pipeline::stencil_variant::{ColorVariantSpec, StencilVariant};
use crate::gpu::raster::raster_atlas::raster_quad::RasterQuad;
use crate::gpu::resource::texture_binding::TextureBinding;

/// Shader and group-0 layout, built once and lent to every raster tenant. Atlases keep clones of the layout
/// (a refcount bump), so one pipeline binds against either atlas without relying on structural compatibility.
#[derive(Debug)]
pub(crate) struct RasterProgram {
    shader: wgpu::ShaderModule,
    layout: wgpu::BindGroupLayout,
    pipeline_layout: wgpu::PipelineLayout,
}

impl RasterProgram {
    pub(crate) fn new(device: &wgpu::Device) -> Self {
        let layout = Self::create_layout(device);
        let pipeline_layout =
            PipelineRecipe::pipeline_layout(device, "palantir.raster.pl", &[Some(&layout)]);
        Self {
            shader: ShaderBody::RasterAtlas.module(device),
            layout,
            pipeline_layout,
        }
    }

    pub(super) const fn layout(&self) -> &wgpu::BindGroupLayout {
        &self.layout
    }

    /// Builds the base and stencil-test pipelines against `format`, the only format-dependent part here.
    pub(crate) fn build_variants(
        &self,
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
    ) -> StencilVariant {
        StencilVariant::build(
            device,
            ColorVariantSpec {
                label: "palantir.raster.pipeline",
                stencil_label: "palantir.raster.pipeline.stencil_test",
                shader: &self.shader,
                vertex_entry: "vs",
                fragment_entry: "fs",
                constants: &[],
                layout: &self.pipeline_layout,
                vertex_buffers: &[Some(RasterQuad::instance_layout())],
                topology: wgpu::PrimitiveTopology::TriangleStrip,
            },
            format,
        )
    }

    /// Group 0: mask at 0, colour at 1, no sampler (the shader reads each texel by index).
    fn create_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
        device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("palantir.raster.atlas layout"),
            entries: &[
                TextureBinding::texture_entry(0),
                TextureBinding::texture_entry(1),
            ],
        })
    }
}
