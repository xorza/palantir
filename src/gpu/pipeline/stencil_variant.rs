//! A colour pipeline and its stencil-test twin, built together.

use crate::gpu::pipeline::pipeline_recipe::PipelineRecipe;
use crate::gpu::surface::stencil::Stencil;

/// A color render pipeline paired with its stencil-test twin (the same
/// recipe plus [`Stencil::test_state`]).
/// `base` runs on plain frames; `test` runs in the stencil-attached
/// rounded-clip pass. Shared by the quad / mesh / image / curve
/// pipelines so base-vs-test selection can't drift across them. Both
/// are built up front so a
/// [`FormatPipelines`](crate::gpu::pipeline::format_pipelines::FormatPipelines)
/// set is complete the moment it exists.
#[derive(Debug)]
pub(crate) struct StencilVariant {
    base: wgpu::RenderPipeline,
    test: wgpu::RenderPipeline,
}

/// What one color-pipeline family varies: labels, shader, entry points,
/// override constants, pipeline layout, vertex buffers, topology. Everything else
/// (`ColorWrites::ALL`, premultiplied blend) is fixed across the
/// quad / mesh / image / curve / raster families and filled in by
/// [`StencilVariant::build`].
///
/// The layout arrives built rather than described, because a family with
/// pipelines outside this pair — quad, with its two mask variants —
/// shares one layout across all of them. A layout carries no
/// depth-stencil state and no fragment entry, so every pipeline of one
/// family wants the same object.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ColorVariantSpec<'a> {
    pub(crate) label: &'static str,
    pub(crate) stencil_label: &'static str,
    pub(crate) shader: &'a wgpu::ShaderModule,
    /// The vertex entry point: `vs` everywhere but quad's shadows, whose
    /// `vs_shadow` also reads their cutout tables.
    pub(crate) vertex_entry: &'static str,
    /// The fragment entry point. A pipeline compiles only what its entry
    /// reaches, so one module can hold a path its other entries leave out:
    /// quad's shadows, in `fs_shadow`, stay out of `fs`.
    pub(crate) fragment_entry: &'static str,
    /// Values for the shader's `override` constants, by name. Empty for
    /// the module's defaults.
    pub(crate) constants: &'a [(&'a str, f64)],
    pub(crate) layout: &'a wgpu::PipelineLayout,
    pub(crate) vertex_buffers: &'a [Option<wgpu::VertexBufferLayout<'a>>],
    pub(crate) topology: wgpu::PrimitiveTopology,
}

impl StencilVariant {
    /// Build the base + stencil-test twin for one swapchain format from
    /// one spec. Shared by every color family's `build_variants` so they
    /// cannot drift on blend or writes.
    pub(crate) fn build(
        device: &wgpu::Device,
        spec: ColorVariantSpec<'_>,
        color_format: wgpu::TextureFormat,
    ) -> Self {
        let variant = |label: &'static str, depth_stencil: Option<wgpu::DepthStencilState>| {
            PipelineRecipe {
                label,
                shader: spec.shader,
                layout: spec.layout,
                vertex_entry: spec.vertex_entry,
                constants: spec.constants,
                vertex_buffers: spec.vertex_buffers,
                topology: spec.topology,
                color_format,
                fragment_entry: spec.fragment_entry,
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
    pub(crate) const fn select(&self, use_stencil: bool) -> &wgpu::RenderPipeline {
        if use_stencil { &self.test } else { &self.base }
    }
}
