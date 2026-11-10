//! A color pipeline and its twin that takes the rounded clips, each built on the first frame that
//! draws through it.

use crate::gpu::device::gpu::Gpu;
use crate::gpu::pipeline::clip_stencil;
use crate::gpu::pipeline::pipeline_recipe::{Pipeline, PipelineRecipe};

/// Whether a frame clips to a rounded rect, which picks the half of a [`ClipVariant`] its passes
/// bind.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Clipping {
    /// No rounded clip: no stencil attachment, and the pipelines that test none.
    Off,
    /// A rounded clip: its passes attach the stencil and bind the stencil-test twins.
    On,
}

/// A color render pipeline in two halves built from one recipe: `off`, and `on`, the same recipe
/// plus [`clip_stencil::test_state`], which runs in the stencil-attached rounded-clip pass. Shared
/// by the quad / mesh / image / curve / raster pipelines so the choice of half can't drift.
///
/// Each half is built on the first frame that draws through it ([`Self::ensure`]), as the frame's
/// census asks before its passes record, so an app that never clips to a rounded rect builds no
/// twin. Refused: both built up front, which made every format compile its twins, at startup and
/// again at each device recovery.
#[derive(Clone, Debug)]
pub(crate) struct ClipVariant {
    off: Option<Pipeline>,
    on: Option<Pipeline>,
}

impl ClipVariant {
    /// Neither half built.
    pub(crate) const NONE: Self = Self {
        off: None,
        on: None,
    };

    /// Build the half `clipping` binds, where it is not built: `recipe`, or under
    /// [`Clipping::On`] its twin, the same recipe labeled `{label}.stencil_test` with
    /// [`clip_stencil::test_state`]. Shared by every color family so the twins cannot drift.
    pub(crate) fn ensure(&mut self, gpu: &Gpu, clipping: Clipping, recipe: &PipelineRecipe<'_>) {
        debug_assert!(
            recipe.depth_stencil.is_none(),
            "the base of a stencil pair tests no stencil"
        );
        match clipping {
            Clipping::Off => {
                if self.off.is_none() {
                    self.off = Some(recipe.clone().build(gpu));
                }
            }
            Clipping::On => {
                if self.on.is_none() {
                    let label = format!("{}.stencil_test", recipe.label);
                    let test = PipelineRecipe {
                        label: &label,
                        depth_stencil: Some(clip_stencil::test_state()),
                        ..recipe.clone()
                    };
                    self.on = Some(test.build(gpu));
                }
            }
        }
    }

    /// The pipeline a frame of `clipping` binds, which its census built.
    pub(crate) const fn get(&self, clipping: Clipping) -> &Pipeline {
        let built = match clipping {
            Clipping::Off => &self.off,
            Clipping::On => &self.on,
        };
        built
            .as_ref()
            .expect("the frame's census builds every pipeline it binds")
    }
}

#[cfg(test)]
pub(crate) mod internals {
    use crate::gpu::pipeline::clip_variant::{ClipVariant, Clipping};

    impl ClipVariant {
        /// Whether the half `clipping` binds is built.
        pub(crate) const fn is_built(&self, clipping: Clipping) -> bool {
            match clipping {
                Clipping::Off => self.off.is_some(),
                Clipping::On => self.on.is_some(),
            }
        }
    }
}
