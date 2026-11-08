//! A color pipeline and its stencil-test twin, built together.

use crate::gpu::device::gpu::Gpu;
use crate::gpu::pipeline::clip_stencil;
use crate::gpu::pipeline::pipeline_recipe::{Pipeline, PipelineRecipe};

/// A color render pipeline paired with its stencil-test twin (the same recipe plus
/// [`clip_stencil::test_state`]). `base` runs on plain frames; `test` runs in the stencil-attached
/// rounded-clip pass. Shared by the quad / mesh / image / curve / raster pipelines so base-vs-test
/// selection can't drift.
///
/// Each half is built on the first frame that draws through it ([`Self::ensure`]), as the frame's
/// census asks before its passes record, so an app that never clips to a rounded rect builds no
/// twin. Refused: both built up front, which made every format compile its twins, at startup and
/// again at each device recovery.
#[derive(Clone, Debug)]
pub(crate) struct StencilVariant {
    base: Option<Pipeline>,
    test: Option<Pipeline>,
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
    /// Neither half built.
    pub(crate) const NONE: Self = Self {
        base: None,
        test: None,
    };

    /// Build the half a pass of `mode` binds, where it is not built: `recipe`, or under
    /// [`StencilMode::Test`] its twin, the same recipe labeled `{label}.stencil_test` with
    /// [`clip_stencil::test_state`]. Shared by every color family so the twins cannot drift.
    pub(crate) fn ensure(&mut self, gpu: &Gpu, mode: StencilMode, recipe: &PipelineRecipe<'_>) {
        debug_assert!(
            recipe.depth_stencil.is_none(),
            "the base of a stencil pair tests no stencil"
        );
        match mode {
            StencilMode::Off => {
                if self.base.is_none() {
                    self.base = Some(recipe.clone().build(gpu));
                }
            }
            StencilMode::Test => {
                if self.test.is_none() {
                    let label = format!("{}.stencil_test", recipe.label);
                    let test = PipelineRecipe {
                        label: &label,
                        depth_stencil: Some(clip_stencil::test_state()),
                        ..recipe.clone()
                    };
                    self.test = Some(test.build(gpu));
                }
            }
        }
    }

    /// The pipeline to bind in a pass of `mode`, which the frame's census built.
    pub(crate) const fn select(&self, mode: StencilMode) -> &Pipeline {
        let built = match mode {
            StencilMode::Off => &self.base,
            StencilMode::Test => &self.test,
        };
        built
            .as_ref()
            .expect("the frame's census builds every pipeline it binds")
    }
}

#[cfg(test)]
pub(crate) mod internals {
    use crate::gpu::pipeline::stencil_variant::{StencilMode, StencilVariant};

    impl StencilVariant {
        /// Whether the half a pass of `mode` binds is built.
        pub(crate) const fn is_built(&self, mode: StencilMode) -> bool {
            match mode {
                StencilMode::Off => self.base.is_some(),
                StencilMode::Test => self.test.is_some(),
            }
        }
    }
}
