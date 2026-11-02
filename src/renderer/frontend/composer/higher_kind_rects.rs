//! Per-group overlap tracking for the replay tiers above text (every [`PaintTier`]). A query that finds nothing does not flush, so lists are never cleared between queries; a tier scans linearly up to [`LINEAR_RECTS`], then a query lists the rects not yet indexed in a [`TileIndex`] over its own rects and tests only the tiles it touches. Indexed by queries, not pushes, since most tiers are never queried at length.

use crate::common::counters::TestOnly;
use crate::primitives::geometry::urect::URect;
use crate::renderer::frontend::composer::tile_index::TileIndex;
use crate::renderer::render_buffer::paint_tier::PaintTier;
use glam::UVec2;
use std::cell::Cell;

/// The most rects a tier scans without its index.
const LINEAR_RECTS: usize = 32;

#[derive(Debug, Default)]
pub(super) struct HigherKindRects {
    /// One slot per [`PaintTier`], indexed by `PaintTier::idx`; an array because every operation folds over the tiers in `Ord` order.
    tiers: [TierRects; PaintTier::COUNT],
    union: URect,
}

#[derive(Debug, Default)]
struct TierRects {
    rects: Vec<URect>,
    union: URect,
    /// The rects `rects[..indexed]` that paint, by tile, as indices into `rects`. Shaped by the first index, not by [`Self::start_frame`]: most tiers never pass [`LINEAR_RECTS`], and shaping each would allocate a viewport of tiles nothing reads.
    tiles: TileIndex,
    indexed: usize,
    /// The viewport [`Self::start_frame`] gave, until the first index shapes `tiles` to it.
    unshaped: Option<UVec2>,
    intersect_tests: TestOnly<Cell<u32>>,
}

impl TierRects {
    fn start_frame(&mut self, viewport: UVec2) {
        self.clear();
        self.unshaped = Some(viewport);
        self.intersect_tests.reset();
    }

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
        if let Some(viewport) = self.unshaped.take() {
            self.tiles.start_frame(viewport);
        }
        for (at, unindexed) in self.rects.iter().enumerate().skip(self.indexed) {
            if !unindexed.is_paint_empty() {
                let span = self.tiles.span(unindexed.min, unindexed.max());
                self.tiles.insert(at as u32, span);
            }
        }
        self.indexed = self.rects.len();
        if rect.is_paint_empty() {
            return false;
        }
        self.tiles
            .any_over(self.tiles.span(rect.min, rect.max()), |at| {
                self.intersect_tests.bump_shared();
                self.rects[at as usize].intersects(rect)
            })
    }

    fn clear(&mut self) {
        self.rects.clear();
        self.union = URect::ZERO;
        if self.indexed > 0 {
            self.tiles.clear();
            self.indexed = 0;
        }
    }
}

impl HigherKindRects {
    /// Drop what every tier holds, and give each the `viewport` its tiles take on its first index; called before new scene output: at compose start and after a clear-fold discard.
    pub(super) fn start_frame(&mut self, viewport: UVec2) {
        for tier in &mut self.tiers {
            tier.start_frame(viewport);
        }
        self.union = URect::ZERO;
    }

    #[inline]
    pub(super) fn push(&mut self, tier: PaintTier, rect: URect) {
        self.tiers[tier.idx()].push(rect);
        self.union = self.union.union(rect);
    }

    /// Whether painting `incoming` over `rect` would land under something already recorded (the group-flush test): a draw conflicts with the tiers that sort above it, since the backend replays in `PaintTier::ALL` order.
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
pub(crate) mod internals {
    use crate::internals::scratch_report;
    use crate::renderer::frontend::composer::higher_kind_rects::{HigherKindRects, TierRects};

    impl HigherKindRects {
        pub(crate) fn scratch_bytes(&self) -> usize {
            let Self { tiers, union: _ } = self;
            tiers.iter().map(TierRects::scratch_bytes).sum()
        }
    }

    impl TierRects {
        fn scratch_bytes(&self) -> usize {
            let Self {
                rects,
                union: _,
                tiles,
                indexed: _,
                unshaped: _,
                intersect_tests: _,
            } = self;
            scratch_report::vec_bytes(rects) + tiles.scratch_bytes()
        }

        pub(crate) fn intersect_tests(&self) -> u32 {
            self.intersect_tests.count()
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::primitives::geometry::urect::URect;
    use crate::renderer::frontend::composer::higher_kind_rects::{HigherKindRects, TierRects};
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
            // One rect never passes `LINEAR_RECTS`, so no tier shaped its tiles.
            for tier in &rects.tiers {
                assert_eq!(tier.tiles.scratch_bytes(), 0, "recorded={recorded:?}");
            }
        }
    }

    /// A query tests only the rects in its own tiles. 2000 wires, 4 px square on a 16 px lattice in 800 × 640: a 64 px tile holds 16, so 500 labels in the gaps cost 8000 tests, not `500 × 2000 = 10⁶`.
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
        let tests: u32 = rects.tiers.iter().map(TierRects::intersect_tests).sum();
        assert_eq!(tests, 500 * 16);
        for tier in PaintTier::ALL
            .into_iter()
            .filter(|&t| t != PaintTier::Curve)
        {
            assert_eq!(rects.tiers[tier.idx()].tiles.scratch_bytes(), 0, "{tier:?}");
        }

        // The next frame's first index reshapes the tiles, dropping last frame's wires. 33 rects pass `LINEAR_RECTS`, and the corner pixel stretches the union over the first wire, so only the tiles can answer.
        rects.start_frame(glam::UVec2::new(800, 640));
        rects.push(PaintTier::Curve, URect::new(0, 0, 1, 1));
        for _ in 0..32 {
            rects.push(PaintTier::Curve, URect::new(780, 620, 4, 4));
        }
        assert!(!rects.any_overlap(URect::new(2, 2, 4, 4)));
        assert!(rects.any_overlap(URect::new(781, 621, 1, 1)));
    }
}
