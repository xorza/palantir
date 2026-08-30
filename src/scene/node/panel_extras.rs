//! The per-node container column: gaps, justification, child alignment, transform.

use crate::layout::types::align::{Align, HAlign, VAlign};
use crate::layout::types::justify::Justify;
use crate::primitives::approx::FloatHash;
use crate::primitives::translate_scale::TranslateScale;
use crate::scene::node::gaps::Gaps;
use std::hash::Hash;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct PanelExtras {
    pub(crate) gaps: Gaps,
    pub(crate) justify: Justify,
    pub(crate) child_align: Align,
    pub(crate) transform: TranslateScale,
}

impl Hash for PanelExtras {
    #[inline]
    fn hash<H: std::hash::Hasher>(&self, h: &mut H) {
        let gaps_u32 = self.gaps.as_u32();
        let packed = (gaps_u32 as u64)
            | ((self.child_align.raw() as u64) << 32)
            | ((self.justify as u64) << 40);
        h.write_u64(packed);
        if !self.transform.is_identity() {
            h.write_u8(1);
            self.transform.translation.hash_visual(h);
            (self.transform.scale - 1.0).hash_visual(h);
        } else {
            h.write_u8(0);
        }
    }
}

impl PanelExtras {
    pub(crate) const DEFAULT: Self = Self {
        gaps: Gaps::ZERO,
        justify: Justify::Start,
        child_align: Align::new(HAlign::Auto, VAlign::Auto),
        transform: TranslateScale::IDENTITY,
    };

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
