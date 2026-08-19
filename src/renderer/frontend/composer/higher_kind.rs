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
use crate::renderer::render_buffer::paint_tier::PaintTier;

#[derive(Debug, Default)]
pub(super) struct HigherKindRects {
    /// One slot per [`PaintTier`], indexed by `PaintTier::idx`.
    ///
    /// An array rather than four named fields, because every operation
    /// below is a fold over the tiers in `Ord` order — and with named
    /// fields `conflicts` had to spell that order out as a triangular
    /// matrix of six hand-written disjunctions, which is one more copy
    /// of the replay order to keep in step with the backend's.
    tiers: [TierRects; PaintTier::COUNT],
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
        self.union = URect::ZERO;
    }
}

impl HigherKindRects {
    pub(super) fn push(&mut self, tier: PaintTier, rect: URect) {
        self.tiers[tier.idx()].push(rect);
        self.union = self.union.union(rect);
    }

    /// Whether painting `incoming` over `rect` would land under
    /// something already recorded — the group-flush test.
    ///
    /// A draw conflicts with the tiers that paint *after* it, which is
    /// exactly the tiers that sort above it: the backend replays in
    /// `PaintTier::ALL` order, so "recorded and higher" means "already
    /// on top". Reading that off `Ord` rather than restating it as a
    /// matrix is what keeps this end and the schedule's drain order from
    /// drifting.
    pub(super) fn conflicts(&self, incoming: PaintTier, rect: URect) -> bool {
        PaintTier::ALL
            .iter()
            .filter(|&&recorded| incoming < recorded)
            .any(|&recorded| self.tiers[recorded.idx()].any_overlap(rect))
    }

    pub(super) fn any_overlap(&self, rect: URect) -> bool {
        self.union.intersects(rect) && self.tiers.iter().any(|t| t.any_overlap(rect))
    }

    pub(super) fn clear(&mut self) {
        for tier in &mut self.tiers {
            tier.clear();
        }
        self.union = URect::ZERO;
    }
}

#[cfg(test)]
mod tests {
    use crate::primitives::urect::URect;
    use crate::renderer::frontend::composer::higher_kind::HigherKindRects;
    use crate::renderer::render_buffer::paint_tier::PaintTier;

    #[test]
    fn conflict_matrix_matches_replay_order_and_kind_blind_queries() {
        let tiers = [
            PaintTier::Mesh,
            PaintTier::Image,
            PaintTier::Icon,
            PaintTier::Curve,
        ];
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
