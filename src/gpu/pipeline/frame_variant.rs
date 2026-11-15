//! [`FrameVariant`]: what picks a frame's pipelines, and [`VariantTarget`], what one is built for.

use crate::gpu::pipeline::atlas_binding::AtlasSlots;
use crate::gpu::pipeline::clip_variant::Clipping;

/// What picks a frame's pipelines of every format, made once a submit after the pool's commit:
/// whether its draws name rounded clips, and the open page slots of the lists its raster and
/// shadow draws read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct FrameVariant {
    pub(crate) clipping: Clipping,
    pub(crate) slots: AtlasSlots,
}

/// What a pipeline is built for: the target's color format, under the frame's variant.
#[derive(Clone, Copy, Debug)]
pub(crate) struct VariantTarget {
    pub(crate) format: wgpu::TextureFormat,
    pub(crate) variant: FrameVariant,
}

impl FrameVariant {
    /// What a frame of this variant builds its pipelines of `format` for.
    pub(crate) const fn target(self, format: wgpu::TextureFormat) -> VariantTarget {
        VariantTarget {
            format,
            variant: self,
        }
    }
}
