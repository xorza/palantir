//! The GPU program every raster quad draws through, whichever tenant rasterized it: one shader module and one
//! group-0 layout, so one pipeline pair per swapchain format. Each tenant still owns its atlas; a shared
//! layout shares the shape of a binding, not the space behind it. See [`RasterPass`] for the CPU half.
//!
//! [`RasterPass`]: crate::gpu::raster::raster_pass::RasterPass

use crate::gpu::pipeline::pipeline_recipe::PipelineRecipe;
use crate::gpu::pipeline::shader_body::ShaderBody;
use crate::gpu::pipeline::stencil_variant::StencilVariant;
use crate::gpu::raster::raster_atlas::raster_quad::{RasterQuad, SpunRasterQuad};
use crate::gpu::resource::texture_binding::TextureBinding;

/// The two pipeline pairs one swapchain format draws raster quads with.
#[derive(Debug)]
pub(crate) struct RasterVariants {
    /// Unturned quads, drawn texel for texel.
    pub(crate) flat: StencilVariant,
    /// [`SpunRasterQuad`]s, turned and filtered.
    pub(crate) spun: StencilVariant,
}

/// The pipelines and layout one raster draw binds, picked for the pass's stencil mode.
#[derive(Clone, Copy, Debug)]
pub(crate) struct RasterPipelines<'a> {
    pub(crate) flat: &'a wgpu::RenderPipeline,
    pub(crate) spun: &'a wgpu::RenderPipeline,
    pub(crate) layout: &'a wgpu::PipelineLayout,
}

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

    /// The pipeline layout both tenants' pipelines are built over.
    pub(crate) const fn pipeline_layout(&self) -> &wgpu::PipelineLayout {
        &self.pipeline_layout
    }

    pub(super) const fn layout(&self) -> &wgpu::BindGroupLayout {
        &self.layout
    }

    /// Builds the base and stencil-test pipelines against `format`, the only format-dependent part
    /// here: for unturned quads, and for the [`SpunRasterQuad`]s a paint animation turns.
    pub(crate) fn build_variants(
        &self,
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
    ) -> RasterVariants {
        RasterVariants {
            flat: StencilVariant::build(
                device,
                PipelineRecipe {
                    vertex_buffers: &[Some(RasterQuad::instance_layout())],
                    topology: wgpu::PrimitiveTopology::TriangleStrip,
                    ..PipelineRecipe::color(
                        "palantir.raster.pipeline",
                        &self.shader,
                        &self.pipeline_layout,
                        format,
                    )
                },
                "palantir.raster.pipeline.stencil_test",
            ),
            spun: StencilVariant::build(
                device,
                PipelineRecipe {
                    vertex_buffers: &[Some(SpunRasterQuad::instance_layout())],
                    topology: wgpu::PrimitiveTopology::TriangleStrip,
                    vertex_entry: "vs_spun",
                    fragment_entry: "fs_spun",
                    ..PipelineRecipe::color(
                        "palantir.raster.pipeline.spun",
                        &self.shader,
                        &self.pipeline_layout,
                        format,
                    )
                },
                "palantir.raster.pipeline.spun.stencil_test",
            ),
        }
    }

    /// Group 0: mask at 0, color at 1, no sampler (the shader reads each texel by index).
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
