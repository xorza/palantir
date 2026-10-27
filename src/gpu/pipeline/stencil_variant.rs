//! A color pipeline and its stencil-test twin, built together.

use crate::gpu::pipeline::pipeline_recipe::PipelineRecipe;
use crate::gpu::surface::stencil::Stencil;

/// A color render pipeline paired with its stencil-test twin (the same recipe plus
/// [`Stencil::test_state`]). `base` runs on plain frames; `test` runs in the stencil-attached
/// rounded-clip pass. Shared by the quad / mesh / image / curve pipelines so base-vs-test selection
/// can't drift. Both are built up front so a
/// [`FormatPipelines`](crate::gpu::pipeline::format_pipelines::FormatPipelines)
/// set is complete the moment it exists.
#[derive(Debug)]
pub(crate) struct StencilVariant {
    base: wgpu::RenderPipeline,
    test: wgpu::RenderPipeline,
}

/// Whether a pass carries the rounded-clip stencil attachment, which picks a
/// [`StencilVariant`]'s pipeline.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum StencilMode {
    /// No attachment: the base pipeline.
    Off,
    /// The rounded-clip attachment: the stencil-test twin.
    Test,
}

impl StencilVariant {
    /// Build `recipe` and its stencil-test twin, the same recipe under `stencil_label` with
    /// [`Stencil::test_state`]. Shared by every color family so the twins cannot drift.
    pub(crate) fn build(
        device: &wgpu::Device,
        recipe: PipelineRecipe<'_>,
        stencil_label: &'static str,
    ) -> Self {
        debug_assert!(
            recipe.depth_stencil.is_none(),
            "the base of a stencil pair tests no stencil"
        );
        let test = PipelineRecipe {
            label: stencil_label,
            depth_stencil: Some(Stencil::test_state()),
            ..recipe.clone()
        };
        Self {
            base: recipe.build(device),
            test: test.build(device),
        }
    }

    /// The pipeline to bind in a pass of `mode`.
    pub(crate) const fn select(&self, mode: StencilMode) -> &wgpu::RenderPipeline {
        match mode {
            StencilMode::Off => &self.base,
            StencilMode::Test => &self.test,
        }
    }
}
