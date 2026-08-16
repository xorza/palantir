//! Per-group overlap tracking for mesh, image, and curve replay tiers.
//!
//! **Why a union pre-reject plus a linear scan, and not the tiled index
//! [`text_grid`] uses.** That module's own doc measures this exact shape
//! at 56.7 µs against its grid's 7.0 — but it is measuring a different
//! access pattern, and the difference is what makes the cheap structure
//! right here. A text batch spans groups, so its rect list is long-lived
//! and every quad queries it; the tiles are what stop that from being
//! O(n) per quad.
//!
//! These lists are group-scoped, and the first query that *survives* the
//! union pre-reject flushes the group — which clears them. So a scan can
//! only ever happen once per group transition, never once per draw.
//! Measured on the `FrameFixture` workload: max occupancy **2**, and
//! **zero** tier scans across eight frames, because the aggregate union
//! rejects every query before it reaches a tier. Measured on a synthetic
//! 400-wire node-graph canvas (the adversarial case, since
//! [`HigherKindRects::conflicts`] never flushes on curve-after-curve, so
//! wires accumulate unbounded): occupancy tracks the wire count exactly,
//! yet the whole compose does **one** 400-rect scan — the first
//! overlapping quad pays it, flushes, and every later quad scans an empty
//! list.
//!
//! A tiled index would add a per-frame build to save a scan that happens
//! once per group. Re-measure before changing this; don't re-derive it
//! from the neighbouring module's numbers.
//!
//! [`text_grid`]: crate::renderer::frontend::composer::text_grid

use crate::primitives::urect::URect;
use crate::renderer::render_buffer::batch::PaintTier;

#[derive(Debug, Default)]
pub(super) struct HigherKindRects {
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
        self.union.intersects(rect) && self.rects.iter().any(|r| r.intersects(rect))
    }

    fn clear(&mut self) {
        self.rects.clear();
        self.union = URect::default();
    }
}

impl HigherKindRects {
    pub(super) fn push(&mut self, tier: PaintTier, rect: URect) {
        let tier_rects = match tier {
            PaintTier::Mesh => &mut self.meshes,
            PaintTier::Image => &mut self.images,
            PaintTier::Curve => &mut self.curves,
        };
        tier_rects.push(rect);
        self.union = self.union.union(rect);
    }

    pub(super) fn conflicts(&self, incoming: PaintTier, rect: URect) -> bool {
        match incoming {
            PaintTier::Mesh => self.images.any_overlap(rect) || self.curves.any_overlap(rect),
            PaintTier::Image => self.curves.any_overlap(rect),
            PaintTier::Curve => false,
        }
    }

    pub(super) fn any_overlap(&self, rect: URect) -> bool {
        self.union.intersects(rect)
            && (self.meshes.any_overlap(rect)
                || self.images.any_overlap(rect)
                || self.curves.any_overlap(rect))
    }

    pub(super) fn clear(&mut self) {
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
