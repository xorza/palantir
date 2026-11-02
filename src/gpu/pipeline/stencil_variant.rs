//! A color pipeline and its stencil-test twin, built together.

use crate::gpu::device::gpu::Gpu;
use crate::gpu::pipeline::pipeline_recipe::PipelineRecipe;
use crate::renderer::render_buffer::MAX_ROUNDED_CLIP_DEPTH;

/// The rounded-clip stencil's format and the states every rounded-clip-aware pipeline takes.
/// Masks stamp as a depth-counted stack (`mask_stamp`: level `k` writes `k + 1` where the stencil
/// equals `k`), and color draws test at `stencil_reference = chain depth`. The window's
/// attachment, [`Stencil`](crate::gpu::surface::stencil::Stencil), is made in its format.
#[derive(Debug)]
pub(crate) struct ClipStencil;

impl ClipStencil {
    /// `Stencil8` suffices for the mask path; UI is 2D.
    pub(crate) const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Stencil8;

    /// Depth/stencil state for one rounded-clip pipeline; only `compare`,
    /// `pass_op` and `write_mask` differ, so the rest is stated once.
    fn state(
        compare: wgpu::CompareFunction,
        pass_op: wgpu::StencilOperation,
        write_mask: u32,
    ) -> wgpu::DepthStencilState {
        let face = wgpu::StencilFaceState {
            compare,
            fail_op: wgpu::StencilOperation::Keep,
            depth_fail_op: wgpu::StencilOperation::Keep,
            pass_op,
        };
        wgpu::DepthStencilState {
            format: Self::FORMAT,
            depth_write_enabled: Some(false),
            depth_compare: Some(wgpu::CompareFunction::Always),
            stencil: wgpu::StencilState {
                front: face,
                back: face,
                read_mask: MAX_ROUNDED_CLIP_DEPTH,
                write_mask,
            },
            bias: wgpu::DepthBiasState::default(),
        }
    }

    /// The stencil-test color pipelines: ref 0 outside masks, chain depth
    /// inside, compared `Equal`; `write_mask = 0` keeps the stamped masks.
    pub(crate) fn test_state() -> wgpu::DepthStencilState {
        Self::state(
            wgpu::CompareFunction::Equal,
            wgpu::StencilOperation::Keep,
            0x00,
        )
    }

    /// The mask-stamp variant, drawn per level at `stencil_reference = level`:
    /// writes `level + 1` where the SDF passes and the stencil equals `level`.
    pub(crate) fn stamp_state() -> wgpu::DepthStencilState {
        Self::state(
            wgpu::CompareFunction::Equal,
            wgpu::StencilOperation::IncrementClamp,
            MAX_ROUNDED_CLIP_DEPTH,
        )
    }

    /// The mask-clear variant, drawn at reference 0; the outermost quad suffices.
    pub(crate) fn clear_state() -> wgpu::DepthStencilState {
        Self::state(
            wgpu::CompareFunction::Always,
            wgpu::StencilOperation::Replace,
            MAX_ROUNDED_CLIP_DEPTH,
        )
    }
}

/// A color render pipeline paired with its stencil-test twin (the same recipe plus
/// [`ClipStencil::test_state`]). `base` runs on plain frames; `test` runs in the stencil-attached
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
    /// [`ClipStencil::test_state`]. Shared by every color family so the twins cannot drift.
    pub(crate) fn build(
        gpu: &Gpu,
        recipe: PipelineRecipe<'_>,
        stencil_label: &'static str,
    ) -> Self {
        debug_assert!(
            recipe.depth_stencil.is_none(),
            "the base of a stencil pair tests no stencil"
        );
        let test = PipelineRecipe {
            label: stencil_label,
            depth_stencil: Some(ClipStencil::test_state()),
            ..recipe.clone()
        };
        Self {
            base: recipe.build(gpu),
            test: test.build(gpu),
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
