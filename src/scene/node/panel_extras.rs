//! The per-node container column: gaps, justification, child alignment, transform.

use crate::primitives::geometry::translate_scale::TranslateScale;
use crate::primitives::layout::align::{Align, HAlign, VAlign};
use crate::primitives::layout::justify::Justify;
use crate::primitives::math::float_hash::FloatHash;
use crate::scene::node::gaps::Gaps;
use std::hash;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct PanelExtras {
    pub(crate) gaps: Gaps,
    pub(crate) justify: Justify,
    pub(crate) child_align: Align,
    pub(crate) transform: TranslateScale,
}

impl PanelExtras {
    pub(crate) const DEFAULT: Self = Self {
        gaps: Gaps::ZERO,
        justify: Justify::Start,
        child_align: Align::new(HAlign::Auto, VAlign::Auto),
        transform: TranslateScale::IDENTITY,
    };

    /// Feed what measure and arrange read: the gaps, the justification
    /// and the child alignment. The transform is left out, because it
    /// moves no rect — see [`Self::hash_transform`].
    #[inline]
    pub(crate) fn hash_layout<H: hash::Hasher>(&self, h: &mut H) {
        let gaps_u32 = self.gaps.as_u32();
        let packed = u64::from(gaps_u32)
            | (u64::from(self.child_align.raw()) << 32)
            | ((self.justify as u64) << 40);
        h.write_u64(packed);
    }

    /// Feed the transform under visual canonicalization. The caller
    /// feeds nothing for an identity transform, which is what a node
    /// without a panel row carries too.
    #[inline]
    pub(crate) fn hash_transform<H: hash::Hasher>(&self, h: &mut H) {
        self.transform.translation.hash_visual(h);
        (self.transform.scale - 1.0).hash_visual(h);
    }

    #[inline]
    pub(crate) fn is_default(&self) -> bool {
        self.gaps == Self::DEFAULT.gaps
            && self.justify == Self::DEFAULT.justify
            && self.child_align == Self::DEFAULT.child_align
            && self.transform.is_identity()
    }
}

impl Default for PanelExtras {
    fn default() -> Self {
        Self::DEFAULT
    }
}
