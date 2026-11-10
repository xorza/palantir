//! A color pipeline and its twin that takes the rounded clips, each built on the first frame that
//! draws through it.

use crate::gpu::device::gpu::Gpu;
use crate::gpu::pipeline::pipeline_recipe::{Pipeline, PipelineRecipe};

/// Whether a frame's draws name rounded clips, which picks the half of a [`ClipVariant`] its passes
/// bind.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Clipping {
    /// No draw names a chain: the pipelines whose clip coverage the compiler folded away.
    Off,
    /// A draw names a chain: the pipelines that read it.
    On,
}

/// A color render pipeline in two halves built from one recipe: with `PALANTIR_CLIPS` off, which
/// `primitives/wgsl/frame.wgsl` reads to return a clip coverage of 1 the compiler folds away, and
/// on. A frame whose draws name no rounded clip binds the first, as the clip's code cost a whole
/// frame with none 16 to 23 % of its main pass on an Apple GPU, a fragment register load even where
/// it ran no clip (`.notes/research/2026-11-10-rounded-clip.md`). Speed alone: both halves give the
/// same pixels for a draw of no chain.
///
/// Each half is built on the first frame that draws through it ([`Self::ensure`]), as the frame's
/// census asks before its passes record, so an app that never clips to a rounded rect builds no
/// second half. Refused: both built up front, which made every format compile its twins, at
/// startup and again at each device recovery.
#[derive(Clone, Debug)]
pub(crate) struct ClipVariant {
    off: Option<Pipeline>,
    on: Option<Pipeline>,
}

/// The override constant `frame.wgsl` declares.
const CLIPS: &str = "PALANTIR_CLIPS";

impl ClipVariant {
    /// Neither half built.
    pub(crate) const NONE: Self = Self {
        off: None,
        on: None,
    };

    /// Build the half `clipping` binds, where it is not built: `recipe`, its override constants
    /// and `PALANTIR_CLIPS`, the clipping half labeled `{label}.clipped`.
    pub(crate) fn ensure(&mut self, gpu: &Gpu, clipping: Clipping, recipe: &PipelineRecipe<'_>) {
        let (slot, clips, label) = match clipping {
            Clipping::Off => (&mut self.off, 0.0, recipe.label.to_owned()),
            Clipping::On => (&mut self.on, 1.0, format!("{}.clipped", recipe.label)),
        };
        if slot.is_none() {
            let constants: Vec<(&str, f64)> = recipe
                .constants
                .iter()
                .copied()
                .chain([(CLIPS, clips)])
                .collect();
            *slot = Some(
                PipelineRecipe {
                    label: &label,
                    constants: &constants,
                    ..recipe.clone()
                }
                .build(gpu),
            );
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
