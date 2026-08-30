//! A colour pipeline and its stencil-test twin, built together.

use crate::renderer::backend::pipeline_recipe::PipelineRecipe;
use crate::renderer::backend::stencil::Stencil;

/// A color render pipeline paired with its stencil-test twin (the same
/// recipe plus [`Stencil::test_state`]).
/// `base` runs on plain frames; `test` runs in the stencil-attached
/// rounded-clip pass. Shared by the quad / mesh / image / curve
/// pipelines so base-vs-test selection can't drift across them. Both
/// are built up front so a
/// [`FormatPipelines`](crate::renderer::backend::format_pipelines::FormatPipelines)
/// set is complete the moment it exists.
#[derive(Debug)]
pub(super) struct StencilVariant {
    base: wgpu::RenderPipeline,
    test: wgpu::RenderPipeline,
}

/// What one color-pipeline family varies: labels, shader, pipeline
/// layout, vertex buffers, topology. Everything else (fragment entry
/// `"fs"`, `ColorWrites::ALL`, premultiplied blend) is fixed across the
/// quad / mesh / image / curve / raster families and filled in by
/// [`StencilVariant::build`].
///
/// The layout arrives built rather than described, because a family with
/// pipelines outside this pair — quad, with its two mask variants —
/// shares one layout across all of them. A layout carries no
/// depth-stencil state and no fragment entry, so every pipeline of one
/// family wants the same object.
#[derive(Debug)]
pub(super) struct ColorVariantSpec<'a> {
    pub(super) label: &'static str,
    pub(super) stencil_label: &'static str,
    pub(super) shader: &'a wgpu::ShaderModule,
    pub(super) layout: &'a wgpu::PipelineLayout,
    pub(super) vertex_buffers: &'a [Option<wgpu::VertexBufferLayout<'a>>],
    pub(super) topology: wgpu::PrimitiveTopology,
}

impl StencilVariant {
    /// Build the base + stencil-test twin for one swapchain format from
    /// one spec. Shared by every color family's `build_variants` so they
    /// cannot drift on blend / writes / fragment entry.
    pub(super) fn build(
        device: &wgpu::Device,
        spec: ColorVariantSpec<'_>,
        color_format: wgpu::TextureFormat,
    ) -> Self {
        let variant = |label: &'static str, depth_stencil: Option<wgpu::DepthStencilState>| {
            PipelineRecipe {
                label,
                shader: spec.shader,
                layout: spec.layout,
                vertex_buffers: spec.vertex_buffers,
                topology: spec.topology,
                color_format,
                fragment_entry: "fs",
                color_writes: wgpu::ColorWrites::ALL,
                blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                depth_stencil,
            }
            .build(device)
        };
        Self {
            base: variant(spec.label, None),
            test: variant(spec.stencil_label, Some(Stencil::test_state())),
        }
    }

    /// The pipeline to bind: the stencil-test twin in a rounded-clip
    /// pass, otherwise the base.
    pub(super) fn select(&self, use_stencil: bool) -> &wgpu::RenderPipeline {
        if use_stencil { &self.test } else { &self.base }
    }
}
