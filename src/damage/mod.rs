//! Per-frame damage detection, computed in [`Ui::frame`](crate::Ui::frame)
//! after `compute_rollups`, rebuilding the prev-frame snapshot in the same
//! pass. `LayerWalk::classify` reads `DamageEngine.prev` to pick a tier and
//! the tier's arm writes the same key back; the read repeats so a live `Entry`
//! does not pin the map across the dispatch.
//!
//! A node is dirty if its `(authoring-hash, cascade-input)` differs from the
//! `DamageEngine.prev` entry with its `WidgetId`, or it has none (added). A
//! `WidgetId` in `prev` with no node this frame contributes its prev rect
//! (removed). Contributions fold into a [`region::DamageRegion`], which drives
//! the encoder filter and per-pass scissor list.
//!
//! **Row invariant.** `prev` holds entries only for widgets with at least one
//! paint row on their last frame (chrome, a direct shape, or a child marker).
//! Rowless nodes contribute no pixels and are never inserted, hidden
//! containers included; child markers carry zero rects, so a parent painting
//! nothing cannot trip the full-repaint threshold. A rows-to-rowless
//! transition evicts the entry in the same diff loop. Childless nodes whose
//! rows are entirely off-surface are skipped too, so a zoomed-out canvas does
//! not fill the map; the `Tier::SubtreeMoved` arm inserts them the frame
//! a move brings their rows on-surface, so every node painting visible pixels
//! has an entry.
//!
//! **Paint order.** Child markers put the shape/child interleave into each
//! row span and `compute_rollups` folds child identity into `node_hash`, so a
//! z-order change routes its parent to the changed-paints arm, where the row
//! matcher's position map feeds the order-inversion check. Reparenting at an
//! identical rect changes no span or hash, so each snapshot also carries
//! [`NodeSnapshot::parent_key`](crate::damage::node_snapshot::NodeSnapshot::parent_key);
//! a mismatch damages the moved subtree's painted extent.
//!
//! `DamageEngine.counters.dirty` is the per-node dirty list in pre-order paint
//! order, a [`TestOnly`](crate::common::counters::TestOnly) cell so production
//! and bench builds skip the per-node push.

use crate::damage::region::CollapsedDamage;
use crate::primitives::geometry::rect::Rect;

#[cfg(feature = "bench")]
pub(crate) mod bench;
pub(crate) mod counters;
pub(crate) mod engine;
pub(crate) mod frame_baseline;
pub(crate) mod inverted_overlaps;
pub(crate) mod node_snapshot;
pub(crate) mod region;
pub(crate) mod root_order;
pub(crate) mod row_matcher;
mod walk;

/// Coverage fraction above which [`Damage::new`] collapses straight to
/// [`Damage::Full`]: past this, per-node filter, per-pass scissor, `Load` and
/// backbuffer-copy bookkeeping costs more than redrawing everything. Checked
/// against the coverage `DamageRegion::collapse_from` measured. The renderer's
/// own lower promote threshold (`DIRECT_PROMOTE_COVERAGE`) is a present-path
/// call kept out of this one.
///
/// It can sit this high because the region keeps disjoint rects disjoint, so
/// `coverage` is the sum of per-rect areas, not one bounding union: two tiny
/// corners score near 0 %.
pub(crate) const FULL_REPAINT_THRESHOLD: f32 = 0.7;

/// What the GPU should do with this frame. "Nothing changed" is `None`, not a
/// variant; see [`Damage::new`]. Clear colour is a presentation concern
/// stamped in by [`crate::renderer::render_plan::RenderPlan`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Damage {
    Full,
    /// **Invariant:** the region is non-empty; [`Damage::new`] is the only
    /// constructor and returns `None` otherwise.
    Partial(CollapsedDamage),
}

impl Damage {
    /// Classify a collapsed damage set into the frame's paint decision, or
    /// `None` when nothing paints. Pure dispatch on the measured coverage
    /// ([`DamageRegion::collapse_from`] checks the degenerate surface).
    /// Absence rather than a variant keeps a no-op out of
    /// [`RenderPlan`](crate::renderer::render_plan::RenderPlan).
    ///
    pub(crate) fn new(damage: CollapsedDamage) -> Option<Damage> {
        if damage.region.is_empty() {
            return None;
        }
        if damage.coverage > FULL_REPAINT_THRESHOLD {
            return Some(Damage::Full);
        }
        Some(Damage::Partial(damage))
    }

    /// Whether the frame paints inside damage rects rather than over the whole
    /// surface.
    pub(crate) const fn is_partial(self) -> bool {
        matches!(self, Self::Partial { .. })
    }
}

/// Push one screen rect into the raw-rect buffer, dropping paint-empty rects
/// (child markers, fully clipped shapes). A whole paint span instead goes
/// through `out.extend(rows.screens())`, which applies the same gate.
#[inline]
fn push_screen(out: &mut Vec<Rect>, screen: Rect) {
    if !screen.is_paint_empty() {
        out.push(screen);
    }
}

/// Gated on `test` alone: only the crate's unit tests call it.
#[cfg(test)]
pub(crate) mod internals {
    use crate::damage::Damage;
    use crate::damage::region::DamageRegion;

    impl Damage {
        /// The rects of a partial frame; panics on any other outcome. A
        /// `Damage` carries measured coverage that rect literals cannot know,
        /// so cases compare rects, or match the variant for an outcome.
        pub(crate) fn expect_partial(damage: Option<Self>) -> DamageRegion {
            match damage {
                Some(Damage::Partial(damage)) => damage.region,
                other => panic!("expected partial damage, got {other:?}"),
            }
        }
    }
}

#[cfg(test)]
mod tests;
