//! Per-frame damage detection. Computed in [`Ui::post_record`] after
//! `compute_hashes`; rebuilds the prev-frame snapshot in the same
//! pass via the `entry()` API — vacant slots get inserted, occupied
//! slots get diffed and either updated or evicted.
//!
//! A node is **dirty** if its `(rect, authoring-hash)` differs from
//! the entry keyed by the same `WidgetId` in `DamageEngine.prev`, OR it
//! had no entry (added). A `WidgetId` present in `DamageEngine.prev` with
//! no matching node this frame contributes its prev rect (removed).
//! Each contribution is folded into a [`region::DamageRegion`] via
//! its merge policy; the result drives the encoder filter and the
//! per-pass scissor list in the backend.
//!
//! **Painting-only invariant.** `DamageEngine.prev` only holds entries for
//! widgets that painted on their last recorded frame (have chrome OR
//! direct shapes — see `Tree.rollups.paints`). Non-painting nodes
//! contribute zero pixels, so they're skipped on insert. A
//! painting→non-painting transition evicts the entry in the same
//! diff loop; the prev rect contributes (clear those pixels), the
//! curr rect doesn't.
//!
//! `DamageEngine.dirty` is the per-node dirty list (added /
//! hash-changed / rect-changed) in pre-order paint order. It's
//! gated behind `cfg(any(test, feature = "internals"))` — production
//! builds skip the per-node `Vec::push` entirely; tests and benches
//! assert on it through this gate.

use crate::forest::Forest;
use crate::forest::rollups::{CascadeInputHash, NodeHash};
#[cfg(any(test, feature = "internals"))]
use crate::forest::tree::NodeId;
use crate::primitives::rect::Rect;
use crate::primitives::widget_id::WidgetId;
use crate::ui::cascade::Cascades;
use crate::ui::damage::region::{DEFAULT_PASS_BUDGET_PX, DamageRegion};
use rustc_hash::{FxHashMap, FxHashSet};
use std::collections::hash_map::Entry;

pub(crate) mod region;

/// Per-painting-widget snapshot held in [`DamageEngine::prev`], keyed by
/// stable [`WidgetId`]. Only widgets that painted last frame have an
/// entry — non-painting nodes (e.g. a popup's invisible click-eater)
/// are skipped on insert, so their full-surface rect can't trip the
/// full-repaint coverage threshold on add or remove. The diff in
/// [`DamageEngine::compute`] reads the prev value and either updates or
/// evicts it in place.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct NodeSnapshot {
    /// Screen-space rect from last frame's `Cascade.paint_rect`
    /// (raw transformed rect inflated by per-shape ink overhang —
    /// drop-shadow halos — then intersected with the ancestor clip).
    /// Using `paint_rect` rather than `visible_rect` means a node
    /// going away (e.g. on tab switch) contributes the full halo
    /// it painted last frame, so the encoder clears the shadow bleed.
    pub(crate) rect: Rect,
    /// Authoring hash from last frame's `Tree.rollups.node`.
    pub(crate) hash: NodeHash,
    /// Rollup hash of this node + its entire subtree from last frame's
    /// `Tree.rollups.subtree`. Pair with `cascade_input` to drive the
    /// subtree-skip fast path: if both match the current frame, every
    /// descendant is bit-identical and the per-node diff can jump to
    /// `subtree_end[i]`.
    pub(crate) subtree_hash: NodeHash,
    /// Fingerprint of last frame's cascade inputs at this node (parent
    /// transform/clip/disabled/invisible + own arranged rect). See
    /// [`crate::forest::rollups::CascadeInputHash`].
    pub(crate) cascade_input: CascadeInputHash,
}

/// Output of one frame's damage pass plus the cross-frame state it
/// reads to produce that output.
///
/// `dirty` lists every added / hash-changed / rect-changed node in
/// pre-order paint order (test-only). `region` accumulates the
/// per-rect contributions (added node's curr rect, changed node's
/// prev + curr, removed widget's prev rect) through
/// [`region::DamageRegion::add`]'s merge policy — empty region ⇒
/// nothing changed, so the host-requested redraw maps to
/// [`Damage::Skip`].
///
/// `prev` is the per-`WidgetId` snapshot map carried over from last
/// frame; it's mutated in place during `compute` (read old, write
/// new) so steady-state frames don't allocate.
///
/// Capacities on `dirty` and `prev` are retained across frames;
/// `region` is inline (`DamageRegion` is `Copy`).
pub(crate) struct DamageEngine {
    #[cfg(any(test, feature = "internals"))]
    pub(crate) dirty: Vec<NodeId>,
    pub(crate) region: DamageRegion,
    /// Per-pass merge budget (extra-overdraw px) used when
    /// `compute` builds the next frame's region. Defaults to
    /// [`DEFAULT_PASS_BUDGET_PX`]; override in place (e.g. from a
    /// debug-overlay slider, a TBDR backend init, or a test) before
    /// the next `Ui::post_record` runs.
    pub(crate) budget_px: f32,
    /// Last frame's snapshot, **only for widgets that painted last
    /// frame** (see the painting-only invariant in the module doc).
    /// Read by the diff in `compute`, then updated/inserted/evicted
    /// in place per node. Cross-layer uniqueness of `WidgetId` is
    /// already enforced by `SeenIds::record` at recording time, so
    /// the bare `WidgetId` key is safe.
    pub(crate) prev: FxHashMap<WidgetId, NodeSnapshot>,
    /// Count of subtree-skip jumps the last `compute` performed —
    /// every match of the Occupied-equal arm jumped `subtree_end - i`
    /// instead of advancing by 1. Read by tests and benches via
    /// `support::internals::damage_subtree_skips`; zero on first
    /// frame and on full-repaint fall-through. Gated alongside
    /// `dirty` — production builds don't pay the increment.
    #[cfg(any(test, feature = "internals"))]
    pub(crate) subtree_skips: u32,
}

impl Default for DamageEngine {
    fn default() -> Self {
        Self {
            #[cfg(any(test, feature = "internals"))]
            dirty: Vec::new(),
            region: DamageRegion::default(),
            budget_px: DEFAULT_PASS_BUDGET_PX,
            prev: FxHashMap::default(),
            #[cfg(any(test, feature = "internals"))]
            subtree_skips: 0,
        }
    }
}

/// Coverage ratio above which the renderer should skip the per-node
/// filter and clear-redraw the whole surface. Below this, the
/// bookkeeping (per-pass scissor + `LoadOp::Load` + backbuffer copy)
/// wins; above it, the savings are eaten by the overhead. The
/// previous 0.5 was tuned for the single-rect-union accumulator
/// where two unrelated tiny corners would blow the union to ~100 %
/// and trip the threshold despite < 1 % of pixels actually
/// changing. The multi-rect region keeps disjoint corners disjoint
/// at the data structure level, so the threshold is now applied to
/// the *sum* of per-rect areas — corner-pair pathologies stay well
/// below 0.7.
pub(crate) const FULL_REPAINT_THRESHOLD: f32 = 0.7;

/// What the GPU should do with this frame:
/// - `None` — nothing changed; the backbuffer is correct as-is.
/// - `Full` — clear + paint everything.
/// - `Partial(region)` — load + scissor; one render pass per rect.
///
/// Knows nothing about clear colour — that's a presentation concern
/// stamped in by [`crate::ui::frame_report::RenderPlan`] when the
/// damage outcome is lifted into a host-facing report.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Damage {
    None,
    Full,
    Partial(DamageRegion),
}

impl Damage {
    /// True iff this is the skip signal — caller can short-circuit
    /// the renderer entirely.
    #[inline]
    pub(crate) fn is_none(self) -> bool {
        matches!(self, Damage::None)
    }
}

impl DamageEngine {
    /// Drop the per-widget previous-frame snapshot map. Pairs with
    /// the caller passing `force_full = true` into the next
    /// `compute` so the diff repopulates the map from scratch but
    /// still returns `Damage::Full`. Called by `Ui::pre_record` when
    /// the surface changed, the previous frame wasn't acked, or
    /// it's the first frame.
    pub(crate) fn invalidate_prev(&mut self) {
        self.prev.clear();
    }

    /// Paint-anim-only damage compute. Caller supplies the
    /// screen-space rects of every paint anim that fired this frame
    /// — typically [`Forest::iter_fired_paint_anim_rects`] — and
    /// this routine unions them into a fresh damage region and
    /// applies the area-coverage filter.
    ///
    /// Does NOT touch `self.prev` (per-widget structural snapshots
    /// stay valid). Caller has already proven via the gate that the
    /// retained tree / cascades / layout are bit-identical to last
    /// frame.
    pub(crate) fn compute_anim_only(
        &mut self,
        fired_rects: impl IntoIterator<Item = Rect>,
        surface: Rect,
    ) -> Damage {
        let mut acc = DamageRegion::with_budget(self.budget_px);
        for r in fired_rects {
            acc.add(r);
        }
        self.region = acc;
        self.filter(surface)
    }

    /// Diff against the just-finished frame and return a
    /// [`Damage`] ready for the renderer:
    ///
    /// - [`Damage::Skip`] — empty region, nothing changed.
    /// - [`Damage::Partial`] — coverage below
    ///   [`FULL_REPAINT_THRESHOLD`].
    /// - [`Damage::Full`] — first frame / surface change /
    ///   degenerate surface / coverage above the threshold.
    ///
    /// `self.prev` is rolled forward in the same pass via the
    /// `entry()` API: vacant slot with a painting node inserts; an
    /// occupied slot whose snapshot is unchanged is a no-op; an
    /// occupied slot whose node still paints but changed updates;
    /// an occupied slot whose node stopped painting is evicted.
    /// Last-frame entries listed in `removed` (precomputed by
    /// [`crate::forest::seen_ids::SeenIds`] so damage and `text` reuse
    /// the diff) are dropped afterwards.
    ///
    /// Rects are tracked in **screen space** (read straight off
    /// `Cascade.paint_rect` — the transformed layout rect inflated by
    /// per-shape ink overhang, then ancestor-clipped). This makes
    /// damage match where the GPU actually paints, so the backend
    /// scissor lands on the right pixels even under transformed
    /// parents or around a drop shadow.
    ///
    /// `surface` is the rect the host arranged the UI into this
    /// frame. A degenerate zero-area surface short-circuits to full
    /// repaint; it shouldn't happen in practice (host filters
    /// resize-to-zero), but cheap to handle.
    #[profiling::function]
    pub(crate) fn compute(
        &mut self,
        forest: &Forest,
        cascades: &Cascades,
        removed: &FxHashSet<WidgetId>,
        surface: Rect,
        force_full: bool,
        fired_anim_rects: impl IntoIterator<Item = Rect>,
    ) -> Damage {
        // `force_full` is the "treat as a fresh frame" signal — set
        // by the caller when `Ui::should_invalidate_prev` decided
        // this frame must repaint everything (surface changed, last
        // frame wasn't acked, or first frame). Caller has already
        // called `invalidate_prev` to drop the per-widget snapshot
        // map; we still run the full diff pass to repopulate it for
        // next frame, just return `Damage::Full` instead of the
        // filtered region.
        #[cfg(any(test, feature = "internals"))]
        {
            self.dirty.clear();
            self.subtree_skips = 0;
        }
        let mut acc = DamageRegion::with_budget(self.budget_px);

        for (layer, tree) in forest.iter_paint_order() {
            let rows = cascades.rows_for(layer);
            let n = tree.records.len();
            let widget_ids = tree.records.widget_id();
            let subtree_end = tree.records.subtree_end();
            let mut i = 0;
            while i < n {
                let wid = widget_ids[i];
                let row = rows[i];
                let curr_rect = row.paint_rect;
                let curr_paints = tree.rollups.paints.contains(i);
                let curr_node_hash = tree.rollups.node[i];
                let curr_subtree_hash = tree.rollups.subtree[i];
                let curr_cascade_input = row.cascade_input;
                let curr = NodeSnapshot {
                    rect: curr_rect,
                    hash: curr_node_hash,
                    subtree_hash: curr_subtree_hash,
                    cascade_input: curr_cascade_input,
                };

                // Invariant: `self.prev` only holds entries for widgets
                // that painted last frame. Non-painting nodes contribute
                // zero rect, so a never-painted widget needs no snapshot
                // and a painting→non-painting transition evicts the
                // entry.
                //
                // Two unchanged predicates, *not* the same:
                //
                // - **No-work** (`rect` + `node_hash` match): this node's
                //   own paint contribution is identical. Falls through to
                //   the next node — descendants may still have changed.
                // - **Subtree-skip** (additionally `subtree_hash` +
                //   `cascade_input` match): the whole subtree is
                //   bit-identical. `subtree_hash` covers every
                //   descendant's `node_hash`; `cascade_input` covers the
                //   ancestor state flowing into this node, so descendant
                //   cascade rows are identical by induction; combined,
                //   every descendant's `(paint_rect, node_hash)` matches
                //   prev. Their prev entries already hold the right
                //   state — no update, no rect contribution, jump to
                //   `subtree_end[i]`.
                //
                // The split matters: if we merged them, an internal node
                // with a stable `(rect, node_hash)` but a child that
                // changed colour would fail the merged predicate (its
                // `subtree_hash` rolled the child's new hash), fall into
                // the "changed" arm, and contribute its own (unchanged)
                // rect to damage — bloating the damage region from the
                // child's leaf rect to the whole parent's rect.
                let (dirty, advance) = match self.prev.entry(wid) {
                    Entry::Vacant(_) if !curr_paints => (false, 1),
                    Entry::Vacant(e) => {
                        e.insert(curr);
                        acc.add(curr_rect);
                        (true, 1)
                    }
                    Entry::Occupied(mut e)
                        if e.get().rect == curr_rect && e.get().hash == curr_node_hash =>
                    {
                        let prev = *e.get();
                        if prev.subtree_hash == curr_subtree_hash
                            && prev.cascade_input == curr_cascade_input
                        {
                            let span = (subtree_end[i] as usize) - i;
                            #[cfg(any(test, feature = "internals"))]
                            if span > 1 {
                                self.subtree_skips += 1;
                            }
                            (false, span)
                        } else {
                            // Own paint unchanged (rect + node_hash
                            // matched), but a descendant or the
                            // cascade input shifted. Refresh those
                            // fields so a later truly-stable frame
                            // can skip; no rect contribution since
                            // this node's own pixels are identical.
                            let snap = e.get_mut();
                            snap.subtree_hash = curr_subtree_hash;
                            snap.cascade_input = curr_cascade_input;
                            (false, 1)
                        }
                    }
                    Entry::Occupied(mut e) => {
                        acc.add(e.get().rect);
                        if curr_paints {
                            acc.add(curr_rect);
                            e.insert(curr);
                        } else {
                            e.remove();
                        }
                        (true, 1)
                    }
                };
                #[cfg(any(test, feature = "internals"))]
                if dirty {
                    self.dirty.push(NodeId(i as u32));
                }
                #[cfg(not(any(test, feature = "internals")))]
                let _ = dirty;
                i += advance;
            }
        }

        // Animation-driven damage. Caller pre-walks the paint-anim
        // registry (see `Forest::iter_fired_paint_anim_rects`) and
        // hands us just the rects that fired this frame. The
        // structural diff above is content-only and (intentionally)
        // doesn't pick up phase flips — bumping
        // `node_hash` / `subtree_hash` would invalidate MeasureCache
        // for the owner's ancestor chain on every flip even though
        // layout didn't change. The encoder's `PaintAnims::sample`
        // decides per-rect whether to emit a quad (visible half) or
        // skip (hidden half).
        for r in fired_anim_rects {
            acc.add(r);
        }

        // Evict last-frame snapshots for removed widgets. Every
        // remaining `prev` entry painted last frame (invariant), so its
        // rect always contributes.
        for wid in removed {
            if let Some(snap) = self.prev.remove(wid) {
                acc.add(snap.rect);
            }
        }

        self.region = acc;
        if force_full {
            return Damage::Full;
        }
        self.filter(surface)
    }

    /// Resolve `self.region` against the area threshold. Empty
    /// region ⇒ `Damage::None` (no widget changed and the surface
    /// is stable; the GPU has nothing to do). Coverage above
    /// [`FULL_REPAINT_THRESHOLD`] (or zero-area surface) ⇒
    /// `Damage::Full`. Otherwise `Damage::Partial(region)`.
    // todo make private expose via internals if needed
    pub(crate) fn filter(&self, surface: Rect) -> Damage {
        if self.region.is_empty() {
            return Damage::None;
        }
        let surface_area = surface.area();
        if surface_area <= 0.0 || self.region.total_area() / surface_area > FULL_REPAINT_THRESHOLD {
            return Damage::Full;
        }
        Damage::Partial(self.region)
    }
}

#[cfg(test)]
mod tests;
