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
//! hash-changed / rect-changed) in pre-order paint order. Always
//! populated; tests assert on it directly, and the "flash dirty
//! nodes" debug overlay (see `docs/roadmap/damage.md`) is the
//! production consumer.

use crate::forest::Forest;
use crate::forest::rollups::NodeHash;
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
    /// Authoring hash from last frame's `Tree.hashes`.
    pub(crate) hash: NodeHash,
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
/// `prev_surface` lets `compute` short-circuit to full repaint on
/// surface change. Backend recreates the backbuffer on resize and
/// force-clears it; if the encoder produced a damage-filtered
/// partial paint instead, the cleared backbuffer would be left as
/// clear color outside the tiny damage scissor.
///
/// Capacities on `dirty` and `prev` are retained across frames;
/// `region` is inline (`DamageRegion` is `Copy`).
pub(crate) struct DamageEngine {
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
    /// Last frame's surface rect. `None` on first frame.
    pub(crate) prev_surface: Option<Rect>,
}

impl Default for DamageEngine {
    fn default() -> Self {
        Self {
            dirty: Vec::new(),
            region: DamageRegion::default(),
            budget_px: DEFAULT_PASS_BUDGET_PX,
            prev: FxHashMap::default(),
            prev_surface: None,
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

/// What the GPU should do with this frame when there *is* work:
/// `Full` (clear + paint everything) or `Partial(region)` (load +
/// scissor; one render pass per rect in the region). The "nothing
/// changed, just present the backbuffer" case is encoded as the
/// absence of a `Damage` — `compute` / `filter` return
/// `Option<Damage>` and `None` is the skip signal.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Damage {
    Full,
    Partial(DamageRegion),
}

impl DamageEngine {
    /// Invalidate the previous-frame snapshot: clears the per-widget
    /// `prev` map and `prev_surface`. `compute` treats
    /// `prev_surface == None` as "force `Damage::Full`" — see
    /// the `force_full` branch — so the next frame paints the whole
    /// surface regardless of the diff. Called by `Ui::pre_record`
    /// when the surface changed, the previous frame wasn't acked, or
    /// it's the first frame.
    pub(crate) fn invalidate_prev(&mut self) {
        self.prev.clear();
        self.prev_surface = None;
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
    ) -> Option<Damage> {
        // `prev_surface == None` is the "treat as a fresh frame"
        // signal. `Ui::pre_record` clears it (and `prev`) when the
        // surface changed, the previous `FrameOutput` wasn't acked,
        // or it's the very first frame; this `compute` doesn't need
        // to repeat that detection. Always update for the next
        // frame's pre_record comparison.
        let force_full = self.prev_surface.is_none();
        self.prev_surface = Some(surface);
        self.dirty.clear();
        let mut acc = DamageRegion::with_budget(self.budget_px);

        for (layer, tree) in forest.iter_paint_order() {
            let rows = cascades.rows_for(layer);
            let n = tree.records.len();
            let widget_ids = tree.records.widget_id();
            for i in 0..n {
                let wid = widget_ids[i];
                let curr_rect = rows[i].paint_rect;
                let curr_paints = tree.rollups.paints.contains(i);
                let curr = NodeSnapshot {
                    rect: curr_rect,
                    hash: tree.rollups.node[i],
                };

                // Invariant: `self.prev` only holds entries for widgets
                // that painted last frame. Non-painting nodes contribute
                // zero rect, so a never-painted widget needs no snapshot
                // and a painting→non-painting transition evicts the
                // entry. Hash equality across an Occupied match implies
                // the same paint contribution (chrome + shapes are
                // hashed), so the unchanged arm needs no explicit
                // `curr_paints` check.
                let dirty = match self.prev.entry(wid) {
                    Entry::Vacant(_) if !curr_paints => false,
                    Entry::Vacant(e) => {
                        e.insert(curr);
                        acc.add(curr_rect);
                        true
                    }
                    Entry::Occupied(e) if *e.get() == curr => false,
                    Entry::Occupied(mut e) => {
                        acc.add(e.get().rect);
                        if curr_paints {
                            acc.add(curr_rect);
                            e.insert(curr);
                        } else {
                            e.remove();
                        }
                        true
                    }
                };
                if dirty {
                    self.dirty.push(NodeId(i as u32));
                }
            }
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
            return Some(Damage::Full);
        }
        self.filter(surface)
    }

    /// Resolve `self.region` against the area threshold. Empty
    /// region ⇒ `None` (no widget changed and the surface is
    /// stable; the GPU has nothing to do). Coverage above
    /// [`FULL_REPAINT_THRESHOLD`] (or zero-area surface) ⇒
    /// `Some(Full)`. Otherwise `Some(Partial(region))`.
    pub(crate) fn filter(&self, surface: Rect) -> Option<Damage> {
        if self.region.is_empty() {
            return None;
        }
        let surface_area = surface.area();
        if surface_area <= 0.0 || self.region.total_area() / surface_area > FULL_REPAINT_THRESHOLD {
            return Some(Damage::Full);
        }
        Some(Damage::Partial(self.region))
    }
}

#[cfg(test)]
mod tests;
