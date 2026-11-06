//! The per-node container column: gaps, justification, child alignment, transform.

use crate::common::content_hash::ContentHash;
use crate::common::hash::Hasher;
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

    /// Feed what measure and arrange read: gaps, justification, child alignment. The transform moves no rect, so it is left out; see [`Self::hash_transform`].
    #[inline]
    pub(crate) fn hash_layout<H: hash::Hasher>(&self, h: &mut H) {
        let gaps_u32 = self.gaps.as_u32();
        let packed = u64::from(gaps_u32)
            | (u64::from(self.child_align.raw()) << 32)
            | ((self.justify as u64) << 40);
        h.write_u64(packed);
    }

    /// Feed the transform under visual canonicalization. Callers feed nothing for an identity transform, which is what a node without a panel row carries.
    #[inline]
    pub(crate) fn hash_transform<H: hash::Hasher>(&self, h: &mut H) {
        self.transform.translation().hash_visual(h);
        (self.transform.scale() - 1.0).hash_visual(h);
    }

    /// The transform on its own, what a direct shape's paint row folds in; the default hash for an
    /// identity transform, so a row under none keeps its shape's hash.
    #[inline]
    pub(crate) fn transform_hash(&self) -> ContentHash {
        if self.transform.is_identity() {
            return ContentHash::default();
        }
        let mut h = Hasher::new();
        self.hash_transform(&mut h);
        ContentHash::finish(&h)
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

#[cfg(test)]
mod tests {
    use crate::common::content_hash::ContentHash;
    use crate::primitives::geometry::translate_scale::TranslateScale;
    use crate::scene::node::panel_extras::PanelExtras;
    use glam::Vec2;

    /// No transform leaves a row's hash as it is, and a move and a scale each read as their own.
    #[test]
    fn transform_hash_is_the_default_for_identity_alone() {
        let hash = |transform| {
            PanelExtras {
                transform,
                ..PanelExtras::DEFAULT
            }
            .transform_hash()
        };
        assert_eq!(hash(TranslateScale::IDENTITY), ContentHash::default());
        let moved = hash(TranslateScale::from_translation(Vec2::new(5.0, 0.0)));
        let scaled = hash(TranslateScale::new(Vec2::ZERO, 1.5));
        assert_ne!(moved, ContentHash::default());
        assert_ne!(scaled, ContentHash::default());
        assert_ne!(moved, scaled);
    }
}
