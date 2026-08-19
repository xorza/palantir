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
pub(crate) struct StencilVariant {
    base: wgpu::RenderPipeline,
    test: wgpu::RenderPipeline,
}

/// What one color-pipeline family varies: labels, shader, bind-group
/// layouts, vertex buffers, topology. Everything else (fragment entry
/// `"fs"`, `ColorWrites::ALL`, premultiplied blend) is fixed across the
/// quad / mesh / image / curve families and filled in by
/// [`StencilVariant::build`].
#[derive(Debug)]
pub(super) struct ColorVariantSpec<'a> {
    pub label: &'static str,
    pub stencil_label: &'static str,
    pub layout_label: &'static str,
    pub shader: &'a wgpu::ShaderModule,
    pub bind_group_layouts: &'a [Option<&'a wgpu::BindGroupLayout>],
    pub vertex_buffers: &'a [Option<wgpu::VertexBufferLayout<'a>>],
    pub topology: wgpu::PrimitiveTopology,
}

impl StencilVariant {
    /// Build the base + stencil-test twin for one swapchain format from
    /// one spec. Shared by the four color families' `build_variants` so
    /// they can't drift on blend / writes / fragment entry — and the
    /// twins share one `PipelineLayout` (depth-stencil state isn't part
    /// of the layout, so building two identical ones was pure waste).
    pub(super) fn build(
        device: &wgpu::Device,
        spec: ColorVariantSpec<'_>,
        color_format: wgpu::TextureFormat,
    ) -> Self {
        let layout =
            PipelineRecipe::pipeline_layout(device, spec.layout_label, spec.bind_group_layouts);
        let variant = |label: &'static str, depth_stencil: Option<wgpu::DepthStencilState>| {
            PipelineRecipe {
                label,
                shader: spec.shader,
                layout: &layout,
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
