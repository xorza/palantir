//! Per-group overlap tracking for the replay tiers above text — every
//! [`PaintTier`].
//!
//! A query that finds nothing does not flush, so nothing clears a tier's
//! list between queries: a canvas of 2000 short wires with 500 labels in
//! the gaps between them keeps every wire for every label, and a flat
//! scan there costs each label the whole wire list. So a tier scans its
//! list only while it holds at most [`LINEAR_RECTS`]; past that, a query
//! indexes the rects no earlier query indexed into a [`RectGrid`] and
//! tests only the ones in its tiles.
//!
//! The index is built by queries, not pushes, because most tiers are
//! never queried at length: a run of curves only pushes, and indexing
//! each push would pay the tile walk for a query that never comes. Each
//! rect is indexed at most once a group, so a frame's indexing is
//! bounded by its rect count.
//!
//! [`RectGrid`]: crate::renderer::frontend::composer::rect_grid::RectGrid

use crate::primitives::urect::URect;
use crate::renderer::frontend::composer::rect_grid::RectGrid;
use crate::renderer::render_buffer::paint_tier::PaintTier;
use glam::UVec2;

/// The most rects a tier scans without its index.
const LINEAR_RECTS: usize = 32;

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
    /// The rects `rects[..indexed]`, by tile.
    grid: RectGrid,
    indexed: usize,
}

impl TierRects {
    fn push(&mut self, rect: URect) {
        self.rects.push(rect);
        self.union = self.union.union(rect);
    }

    /// The union reject inline, since every higher-kind draw asks it of
    /// each tier above its own; the scan behind it out of line.
    #[inline]
    fn any_overlap(&mut self, rect: URect) -> bool {
        self.union.intersects(rect) && self.any_overlap_inside(rect)
    }

    #[inline(never)]
    fn any_overlap_inside(&mut self, rect: URect) -> bool {
        if self.rects.len() <= LINEAR_RECTS {
            return self.rects.iter().any(|r| r.intersects(rect));
        }
        for &unindexed in &self.rects[self.indexed..] {
            self.grid.push(unindexed);
        }
        self.indexed = self.rects.len();
        self.grid.any_overlap(rect)
    }

    fn clear(&mut self) {
        self.rects.clear();
        self.union = URect::ZERO;
        if self.indexed > 0 {
            self.grid.clear();
            self.indexed = 0;
        }
    }
}

impl HigherKindRects {
    /// Size every tier's grid to `viewport` and drop what they hold.
    /// Called at compose start.
    pub(super) fn start_frame(&mut self, viewport: UVec2) {
        for tier in &mut self.tiers {
            tier.grid.start_frame(viewport);
            tier.indexed = 0;
            tier.clear();
        }
        self.union = URect::ZERO;
    }

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
    pub(super) fn conflicts(&mut self, incoming: PaintTier, rect: URect) -> bool {
        PaintTier::ALL
            .iter()
            .filter(|&&recorded| incoming < recorded)
            .any(|&recorded| self.tiers[recorded.idx()].any_overlap(rect))
    }

    pub(super) fn any_overlap(&mut self, rect: URect) -> bool {
        self.union.intersects(rect) && self.tiers.iter_mut().any(|t| t.any_overlap(rect))
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
            rects.start_frame(glam::UVec2::new(64, 64));
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

    /// A query tests only the rects in its own tiles. 2000 wires, 4 px
    /// square on a 16 px lattice, fill 800 × 640 px: every whole 64 px
    /// tile holds a 4 × 4 block of them, 8 in its row and 8 chained. 500
    /// labels sit in the gaps, each inside one whole tile, so each tests
    /// that tile's 16 wires, finds nothing, and does not flush: 8000
    /// tests, where a scan of every wire costs `500 × 2000 = 10⁶`.
    #[test]
    fn labels_between_wires_test_only_their_tile() {
        let mut rects = HigherKindRects::default();
        rects.start_frame(glam::UVec2::new(800, 640));
        for i in 0..50 {
            for j in 0..40 {
                rects.push(PaintTier::Curve, URect::new(16 * i + 2, 16 * j + 2, 4, 4));
            }
        }
        for i in 0..25 {
            for j in 0..20 {
                assert!(!rects.any_overlap(URect::new(16 * i + 9, 16 * j + 9, 6, 6)));
            }
        }
        let tests: u64 = rects.tiers.iter().map(|t| t.grid.intersect_tests()).sum();
        assert_eq!(tests, 500 * 16);
    }
}
