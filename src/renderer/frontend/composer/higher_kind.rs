//! Per-group overlap tracking for mesh, image, and curve replay tiers.

use crate::primitives::urect::URect;
use crate::renderer::render_buffer::batch::PaintTier;

#[derive(Debug, Default)]
pub(crate) struct HigherKindRects {
    meshes: TierRects,
    images: TierRects,
    curves: TierRects,
    union: URect,
}

#[derive(Debug, Default)]
struct TierRects {
    rects: Vec<URect>,
    union: URect,
}

impl TierRects {
    fn push(&mut self, rect: URect) {
        self.rects.push(rect);
        self.union = self.union.union(rect);
    }

    fn any_overlap(&self, rect: URect) -> bool {
        self.union.intersect(rect).is_some()
            && self.rects.iter().any(|r| r.intersect(rect).is_some())
    }

    fn clear(&mut self) {
        self.rects.clear();
        self.union = URect::default();
    }
}

impl HigherKindRects {
    pub(crate) fn push(&mut self, tier: PaintTier, rect: URect) {
        let tier_rects = match tier {
            PaintTier::Mesh => &mut self.meshes,
            PaintTier::Image => &mut self.images,
            PaintTier::Curve => &mut self.curves,
        };
        tier_rects.push(rect);
        self.union = self.union.union(rect);
    }

    pub(crate) fn conflicts(&self, incoming: PaintTier, rect: URect) -> bool {
        match incoming {
            PaintTier::Mesh => self.images.any_overlap(rect) || self.curves.any_overlap(rect),
            PaintTier::Image => self.curves.any_overlap(rect),
            PaintTier::Curve => false,
        }
    }

    pub(crate) fn any_overlap(&self, rect: URect) -> bool {
        self.union.intersect(rect).is_some()
            && (self.meshes.any_overlap(rect)
                || self.images.any_overlap(rect)
                || self.curves.any_overlap(rect))
    }

    pub(crate) fn clear(&mut self) {
        self.meshes.clear();
        self.images.clear();
        self.curves.clear();
        self.union = URect::default();
    }
}

#[cfg(test)]
mod tests {
    use crate::primitives::urect::URect;
    use crate::renderer::frontend::composer::higher_kind::HigherKindRects;
    use crate::renderer::render_buffer::batch::PaintTier;

    #[test]
    fn conflict_matrix_matches_replay_order_and_kind_blind_queries() {
        let tiers = [PaintTier::Mesh, PaintTier::Image, PaintTier::Curve];
        let recorded_rect = URect::new(10, 10, 20, 20);
        let disjoint = URect::new(40, 40, 10, 10);

        for recorded in tiers {
            let mut rects = HigherKindRects::default();
            rects.push(recorded, recorded_rect);
            assert!(rects.any_overlap(recorded_rect), "recorded={recorded:?}");
            assert!(!rects.any_overlap(disjoint), "recorded={recorded:?}");

            for incoming in tiers {
                assert_eq!(
                    rects.conflicts(incoming, recorded_rect),
                    incoming < recorded,
                    "incoming={incoming:?}, recorded={recorded:?}",
                );
                assert!(
                    !rects.conflicts(incoming, disjoint),
                    "incoming={incoming:?}, recorded={recorded:?}",
                );
            }

            rects.clear();
            assert!(!rects.any_overlap(recorded_rect));
        }
    }
}
