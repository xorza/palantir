//! The GPU program every raster quad is drawn through, whichever tenant
//! rasterized it.
//!
//! A glyph and an icon are the same draw — see [`RasterPass`], which is
//! the CPU half of that claim. This is the GPU half: one shader module,
//! one group-0 layout, one sampler, and so **one pipeline pair per
//! swapchain format** rather than one per tenant.
//!
//! What each tenant still owns is the atlas the layout describes — its
//! own textures, its own bind group, its own eviction budget. Sharing a
//! layout shares the *shape* of a binding, not the space behind it, which
//! is why this can be one object while the atlases stay two.
//!
//! [`RasterPass`]: crate::renderer::backend::raster_pass::RasterPass

use crate::renderer::backend::pipeline_recipe::PipelineRecipe;
use crate::renderer::backend::raster_atlas::raster_quad::RasterQuad;
use crate::renderer::backend::stencil_variant::{ColorVariantSpec, StencilVariant};
use crate::renderer::backend::texture_binding;

/// Shader, group-0 layout and sampler, built once and lent to every
/// raster tenant.
///
/// An atlas keeps clones of the layout and the sampler, and a clone of a
/// wgpu handle is a refcount bump onto the same GPU object. So the
/// pipeline built here and every atlas's bind group are not merely
/// *equivalent* layouts, they are one layout — which is what makes one
/// pipeline bindable against either atlas without relying on wgpu's
/// structural-compatibility rules.
#[derive(Debug)]
pub(super) struct RasterProgram {
    shader: wgpu::ShaderModule,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
}

impl RasterProgram {
    pub(super) fn new(device: &wgpu::Device) -> Self {
        Self {
            shader: RasterQuad::shader_module(device, "palantir.raster.shader"),
            layout: Self::create_layout(device),
            sampler: Self::create_sampler(device),
        }
    }

    /// The layout every raster bind group is built against, and every
    /// raster pipeline created with.
    pub(super) fn layout(&self) -> &wgpu::BindGroupLayout {
        &self.layout
    }

    /// The sampler every raster bind group binds at slot 2.
    pub(super) fn sampler(&self) -> &wgpu::Sampler {
        &self.sampler
    }

    /// Build the base and stencil-test pipelines against `format`.
    ///
    /// Format-dependent, and nothing else here is — the shader, the
    /// layout, the sampler and both atlases survive a swapchain
    /// reformat, which is what
    /// [`FormatPipelines`](crate::renderer::backend::format_pipelines::FormatPipelines)
    /// exists to separate.
    pub(super) fn build_variants(
        &self,
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
    ) -> StencilVariant {
        // Group 0 = atlas textures + sampler. Viewport and atlas sizes ride
        // the shared immediate region, so there is no uniform buffer.
        let layout =
            PipelineRecipe::pipeline_layout(device, "palantir.raster.pl", &[Some(&self.layout)]);
        StencilVariant::build(
            device,
            ColorVariantSpec {
                label: "palantir.raster.pipeline",
                stencil_label: "palantir.raster.pipeline.stencil_test",
                shader: &self.shader,
                layout: &layout,
                vertex_buffers: &[Some(RasterQuad::instance_layout())],
                topology: wgpu::PrimitiveTopology::TriangleStrip,
            },
            format,
        )
    }

    /// Group 0: mask at 0, colour at 1, one shared sampler at 2 — the
    /// same entry shapes every other group uses, two textures deep
    /// instead of one.
    fn create_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
        device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("palantir.raster.atlas layout"),
            entries: &[
                texture_binding::texture_entry(0),
                texture_binding::texture_entry(1),
                texture_binding::sampler_entry(2),
            ],
        })
    }

    /// Nearest on every axis: a quad is drawn at its slot's own pixel
    /// dimensions, so every texel maps 1:1 and filtering could only blur
    /// what the rasterizer already got exactly right.
    fn create_sampler(device: &wgpu::Device) -> wgpu::Sampler {
        device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("palantir.raster.sampler"),
            min_filter: wgpu::FilterMode::Nearest,
            mag_filter: wgpu::FilterMode::Nearest,
            mipmap_filter: wgpu::MipmapFilterMode::Nearest,
            ..Default::default()
        })
    }
}
