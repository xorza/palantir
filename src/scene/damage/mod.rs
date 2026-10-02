//! Per-frame damage detection. Computed in [`Ui::frame`](crate::Ui::frame) after
//! `compute_rollups`; rebuilds the prev-frame snapshot in the same
//! pass. `LayerWalk::classify` reads `DamageEngine.prev` to pick a
//! tier, and the arm for that tier writes the same key back —
//! inserting a snapshot, touching one in place, or removing it. The
//! read repeats instead of a live `Entry` crossing the tier dispatch,
//! which would pin the whole map for the arms that never write.
//!
//! A node is **dirty** if its `(authoring-hash, cascade-input)` differs
//! from the entry keyed by the same `WidgetId` in `DamageEngine.prev`,
//! OR it had no entry (added). A `WidgetId` present in
//! `DamageEngine.prev` with no matching node this frame contributes its
//! prev rect (removed).
//! Each contribution is folded into a [`region::DamageRegion`] via
//! its merge policy; the result drives the encoder filter and the
//! per-pass scissor list in the backend.
//!
//! **Row invariant.** `DamageEngine.prev` only holds entries for
//! widgets with at least one paint row on their last recorded frame —
//! chrome, a direct shape, or a child marker (i.e.
//! `cascade.layers[li].paint_arena.node_spans[i].len > 0`). Rowless
//! nodes (childless, chromeless, shapeless) contribute zero pixels and
//! are skipped on insert; child markers carry zero rects, so a parent
//! that paints nothing itself still can't trip the full-repaint
//! coverage threshold on add or remove. A rows→rowless transition
//! evicts the entry in the same diff loop; the prev rects contribute
//! (clear those pixels), the curr rect doesn't.
//!
//! Classification enforces that invariant on the way in — a rowless
//! node is never inserted, a hidden container included — and skips
//! *childless* nodes whose rows are entirely off-surface, so a
//! zoomed-out canvas does not populate the map with thousands of
//! never-visible snapshots. That second skip is repaid in
//! the moved-subtree arm (tier 1.5): the frame a move puts such a
//! node's rows on-surface, its snapshot is inserted there, restoring
//! the induction the prev-extent fold and the removed-widget eviction
//! tail rely on — every node painting *visible* pixels has an entry.
//!
//! **Paint order.** Child markers put the shape/child interleave into
//! each node's row span, and `compute_rollups` folds child identity
//! into `node_hash` — so a pure z-order change (raising a node, a
//! shape crossing a child boundary, two coincident shapes swapping)
//! routes its parent to the changed-paints arm, where the row
//! matcher's position map feeds the order-inversion check and each
//! inverted pair's extent overlap is damaged. Cross-parent moves are
//! the one ordering change no row span or hash captures — a widget
//! reparented (or moved between layers) at an identical rect keeps
//! every hash — so each snapshot also carries
//! [`NodeSnapshot::parent_key`](crate::scene::damage::node_snapshot::NodeSnapshot::parent_key), and a mismatch damages the moved
//! subtree's painted extent.
//!
//! `DamageEngine.counters.dirty` is the per-node dirty list (added /
//! hash- or cascade-changed / evicted) in pre-order paint order. It is a
//! [`TestOnly`](crate::common::counters::TestOnly) cell — `cfg(test)`,
//! so production and bench builds skip the per-node `Vec::push`
//! entirely.

use crate::primitives::rect::Rect;
use crate::scene::damage::region::CollapsedDamage;

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

/// Coverage fraction above which [`Damage::new`] stops tracking partial damage
/// and collapses straight to [`Damage::Full`]: once this much of the surface has
/// changed, the per-node filter + per-pass scissor + `LoadOp::Load` + backbuffer
/// copy bookkeeping costs more than just clearing and redrawing everything.
/// Checked against the coverage `DamageRegion::collapse_from` measured. (The
/// renderer's `DirectAdaptive` strategy applies its own, lower promote threshold
/// to the *Partial* range below this line — `DIRECT_PROMOTE_COVERAGE` in
/// `window_driver` — but that's a present-path GPU-cost call kept out of this
/// damage-tracking one.)
///
/// The threshold can sit this high because the region keeps disjoint rects
/// disjoint at the data-structure level, so `coverage` is the *sum* of
/// per-rect areas rather than the area of one bounding union. Two unrelated
/// tiny corners therefore score near 0 %, not the ~100 % a single-union
/// accumulator would report — which is what a threshold this permissive
/// depends on.
pub(crate) const FULL_REPAINT_THRESHOLD: f32 = 0.7;

/// What the GPU should do with this frame:
/// - `Full` — clear + paint everything.
/// - `Partial(region)` — load + scissor; one render pass per rect.
///
/// "Nothing changed; the backbuffer is correct as-is" is `None`, not a
/// third variant — see [`Damage::new`].
///
/// Knows nothing about clear colour — that's a presentation concern
/// stamped in by [`crate::renderer::render_plan::RenderPlan`] when the
/// damage outcome is lifted into a host-facing report.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Damage {
    Full,
    /// **Invariant:** the wrapped region is non-empty. [`Damage::new`]
    /// is the only constructor and returns `None` when the region is
    /// empty, so consumers can iterate `damage.region.iter_rects()`
    /// without checking `is_empty` first.
    Partial(CollapsedDamage),
}

impl Damage {
    /// Classify a collapsed damage set into the frame's paint decision,
    /// or `None` when the frame paints nothing. Pure dispatch on the
    /// coverage [`DamageRegion::collapse_from`] measured — no surface
    /// needed here; the degenerate-surface check lives at that site.
    ///
    /// "Nothing to paint" is the absence of a `Damage` rather than a
    /// variant of one, so the renderer's plan cannot carry a no-op:
    /// [`RenderPlan`](crate::renderer::render_plan::RenderPlan) holds
    /// this type as-is instead of restating the split in a second enum.
    ///
    /// [`DamageRegion::collapse_from`]: crate::scene::damage::region::DamageRegion::collapse_from
    pub(crate) fn new(damage: CollapsedDamage) -> Option<Damage> {
        if damage.region.is_empty() {
            return None;
        }
        if damage.coverage > FULL_REPAINT_THRESHOLD {
            return Some(Damage::Full);
        }
        Some(Damage::Partial(damage))
    }

    /// Whether the frame paints inside damage rects rather than over the
    /// whole surface.
    ///
    /// Asked of the damage rather than of the `RepaintScissors`
    /// `build_repaint_scissors` maps it one-to-one onto: the scissors
    /// give the same answer one derivation further from the fact.
    pub(crate) const fn is_partial(self) -> bool {
        matches!(self, Self::Partial { .. })
    }
}

/// Push one screen rect into the raw-rect buffer, dropping paint-empty
/// rects — child markers (always zero) and fully clipped-away shapes
/// produce no pixels, so they have nothing to clear or repaint. Lives
/// beside `DamageEngine::raw_rects`, the buffer every caller is filling.
///
/// One rect at a time. A whole paint span goes in through
/// `out.extend(rows.screens())`, whose iterator applies the same gate —
/// those are the two spellings, and neither is written out by hand.
#[inline]
fn push_screen(out: &mut Vec<Rect>, screen: Rect) {
    if !screen.is_paint_empty() {
        out.push(screen);
    }
}

/// In-tree-test-only reach-in. `#[cfg(test)]` rather than the
/// feature-gated `internals` mod, because only the crate's own unit
/// tests call it — so it needs no `allow(dead_code)` for the
/// feature-only build.
#[cfg(test)]
pub(crate) mod test_support {
    use crate::scene::damage::Damage;
    use crate::scene::damage::region::DamageRegion;

    impl Damage {
        /// The rects of a partial frame; panics on any other outcome.
        ///
        /// A `Damage` carries the coverage the frame measured, which an
        /// expected value assembled from rect literals has no way to know
        /// — so a case that means "these rects" compares these, and one
        /// that means "this outcome" matches on the variant.
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
