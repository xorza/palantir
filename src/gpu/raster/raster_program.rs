//! The GPU program every raster quad is drawn through, whichever tenant
//! rasterized it.
//!
//! A glyph and an icon are the same draw — see [`RasterPass`], which is
//! the CPU half of that claim. This is the GPU half: one shader module,
//! one group-0 layout, and so **one pipeline pair per
//! swapchain format** rather than one per tenant.
//!
//! What each tenant still owns is the atlas the layout describes — its
//! own textures, its own bind group, its own eviction budget. Sharing a
//! layout shares the *shape* of a binding, not the space behind it, which
//! is why this can be one object while the atlases stay two.
//!
//! [`RasterPass`]: crate::gpu::raster::raster_pass::RasterPass

use crate::gpu::pipeline::pipeline_recipe::PipelineRecipe;
use crate::gpu::pipeline::shader_body::ShaderBody;
use crate::gpu::pipeline::stencil_variant::{ColorVariantSpec, StencilVariant};
use crate::gpu::raster::raster_atlas::raster_quad::RasterQuad;
use crate::gpu::resource::texture_binding::TextureBinding;

/// Shader and group-0 layout, built once and lent to every raster tenant.
///
/// An atlas keeps a clone of the layout, and a clone of a
/// wgpu handle is a refcount bump onto the same GPU object. So the
/// pipeline built here and every atlas's bind group are not merely
/// *equivalent* layouts, they are one layout — which is what makes one
/// pipeline bindable against either atlas without relying on wgpu's
/// structural-compatibility rules.
#[derive(Debug)]
pub(crate) struct RasterProgram {
    shader: wgpu::ShaderModule,
    layout: wgpu::BindGroupLayout,
    /// Format-independent, so built once here rather than per format.
    pipeline_layout: wgpu::PipelineLayout,
}

impl RasterProgram {
    pub(crate) fn new(device: &wgpu::Device) -> Self {
        let layout = Self::create_layout(device);
        // Group 0 = the atlas textures. The viewport rides the shared
        // immediate region, so there is no uniform buffer.
        let pipeline_layout =
            PipelineRecipe::pipeline_layout(device, "palantir.raster.pl", &[Some(&layout)]);
        Self {
            shader: ShaderBody::RasterAtlas.module(device),
            layout,
            pipeline_layout,
        }
    }

    /// The layout every raster bind group is built against, and every
    /// raster pipeline created with.
    pub(super) const fn layout(&self) -> &wgpu::BindGroupLayout {
        &self.layout
    }

    /// Build the base and stencil-test pipelines against `format`.
    ///
    /// Format-dependent, and nothing else here is — the shader, the
    /// layout and both atlases survive a swapchain
    /// reformat, which is what
    /// [`FormatPipelines`](crate::gpu::pipeline::format_pipelines::FormatPipelines)
    /// exists to separate.
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
                fragment_entry: "fs",
                layout: &self.pipeline_layout,
                vertex_buffers: &[Some(RasterQuad::instance_layout())],
                topology: wgpu::PrimitiveTopology::TriangleStrip,
            },
            format,
        )
    }

    /// Group 0: mask at 0, colour at 1 — the same texture entry every
    /// other group uses, two deep, and no sampler: the shader reads each
    /// texel by index.
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
