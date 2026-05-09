//! Per-frame damage detection. Computed in [`Ui::end_frame`] after
//! `compute_hashes`; rebuilds the prev-frame snapshot in the same
//! pass so the diff reads the old entry and writes the new one per
//! node.
//!
//! A node is **dirty** if its `(rect, authoring-hash)` differs from
//! the entry keyed by the same `WidgetId` in `Damage.prev`, OR it
//! had no entry (added). A `WidgetId` present in `Damage.prev` with
//! no matching node this frame contributes its prev rect (removed).
//! Each contribution is folded into a [`region::DamageRegion`] via
//! its merge policy; the result drives the encoder filter and the
//! per-pass scissor list in the backend.
//!
//! `Damage.dirty` is the per-node dirty list (added /
//! hash-changed / rect-changed) in pre-order paint order. Always
//! populated; tests assert on it directly, and the "flash dirty
//! nodes" debug overlay (see `docs/roadmap/damage.md`) is the
//! production consumer.

use crate::primitives::rect::Rect;
use crate::tree::NodeId;
use crate::tree::forest::Forest;
use crate::tree::node_hash::NodeHash;
use crate::tree::widget_id::WidgetId;
use crate::ui::cascade::CascadeResult;
use crate::ui::damage::region::DamageRegion;
use rustc_hash::FxHashMap;

pub(crate) mod region;

/// Per-widget snapshot retained across frames so the next frame's
/// `Damage::compute` can diff `(rect, hash)` against the previous
/// value. Indexed by stable [`WidgetId`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct NodeSnapshot {
    /// Screen-space rect from last frame's `Cascade.screen_rect`.
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
/// [`DamagePaint::Skip`].
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
#[derive(Default)]
pub(crate) struct Damage {
    pub(crate) dirty: Vec<NodeId>,
    pub(crate) region: DamageRegion,
    /// Last frame's per-widget `(rect, hash)` snapshot. Read by the
    /// diff in `compute`, then rolled forward in the same pass.
    pub(crate) prev: FxHashMap<WidgetId, NodeSnapshot>,
    /// Last frame's surface rect. `None` on first frame.
    pub(crate) prev_surface: Option<Rect>,
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

/// What the GPU should do with this frame. Keeps three cases that
/// were previously squashed into `Option<Rect>` distinct so the
/// backend can branch on them: `Full` (clear + paint everything),
/// `Partial(region)` (load + scissor; one render pass per rect in
/// the region), `Skip` (don't paint — backbuffer already holds the
/// right pixels; just present it).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum DamagePaint {
    Full,
    Partial(DamageRegion),
    Skip,
}

impl Damage {
    /// Invalidate the previous-frame snapshot: clears the per-widget
    /// `prev` map and `prev_surface`. `compute` treats
    /// `prev_surface == None` as "force `DamagePaint::Full`" — see
    /// the `force_full` branch — so the next frame paints the whole
    /// surface regardless of the diff. Called by `Ui::begin_frame`
    /// when the surface changed, the previous frame wasn't acked, or
    /// it's the first frame, and by [`crate::Ui::surface_invalidated`]
    /// for explicit host-driven resets.
    pub(crate) fn invalidate_prev(&mut self) {
        self.prev.clear();
        self.prev_surface = None;
    }

    /// Diff against the just-finished frame and return the filtered
    /// damage rect ready for the encoder filter and the backend
    /// scissor: `Some(rect)` → partial repaint, `None` → full repaint
    /// (no diff, area above [`FULL_REPAINT_THRESHOLD`], or degenerate
    /// `surface`). `self.prev` is rolled forward in the same pass —
    /// the diff reads each `WidgetId`'s old entry via `insert`, then
    /// evicts last-frame entries listed in `removed` (precomputed by
    /// [`crate::ui::seen_ids::SeenIds`] so damage and `text` reuse the diff).
    ///
    /// Rects are tracked in **screen space** (read straight off
    /// `Cascade.screen_rect`). This makes damage match where the GPU
    /// actually paints, so the backend scissor lands on the right
    /// pixels even under transformed parents.
    ///
    /// `surface` is the rect the host arranged the UI into this
    /// frame. A degenerate zero-area surface short-circuits to full
    /// repaint; it shouldn't happen in practice (host filters
    /// resize-to-zero), but cheap to handle.
    pub(crate) fn compute(
        &mut self,
        forest: &Forest,
        cascades: &CascadeResult,
        removed: &[WidgetId],
        surface: Rect,
    ) -> DamagePaint {
        // `prev_surface == None` is the "treat as a fresh frame"
        // signal. `Ui::begin_frame` clears it (and `prev`) when the
        // surface changed, the previous `FrameOutput` wasn't acked,
        // or it's the very first frame; this `compute` doesn't need
        // to repeat that detection. Always update for the next
        // frame's begin_frame comparison.
        let force_full = self.prev_surface.is_none();
        self.prev_surface = Some(surface);
        self.dirty.clear();
        let mut acc = DamageRegion::default();

        for (layer, tree) in forest.iter_paint_order() {
            let rows = cascades.rows_for(layer);
            let n = tree.records.len();
            let widget_ids = tree.records.widget_id();
            for i in 0..n {
                let wid = widget_ids[i];
                let curr_rect = rows[i].screen_rect;
                let curr_hash = tree.rollups.node[i];
                let curr = NodeSnapshot {
                    rect: curr_rect,
                    hash: curr_hash,
                };

                let dirty = match self.prev.insert(wid, curr) {
                    None => {
                        acc.add(curr_rect);
                        true
                    }
                    Some(snap) if snap.hash == curr_hash && snap.rect == curr_rect => false,
                    Some(snap) => {
                        acc.add(snap.rect);
                        acc.add(curr_rect);
                        true
                    }
                };
                if dirty {
                    self.dirty.push(NodeId(i as u32));
                }
            }
        }

        // Evict last-frame snapshots for removed widgets; their rect
        // contributes to damage so the area they vacated repaints.
        for wid in removed {
            if let Some(snap) = self.prev.remove(wid) {
                acc.add(snap.rect);
            }
        }

        self.region = acc;
        if force_full {
            return DamagePaint::Full;
        }
        self.filter(surface)
    }

    /// Resolve `self.region` against the area threshold. Empty
    /// region ⇒ `Skip` (no widget changed and the surface is
    /// stable; the GPU has nothing to do). Coverage above
    /// [`FULL_REPAINT_THRESHOLD`] (or zero-area surface) ⇒ `Full`.
    /// Otherwise `Partial(region)`.
    pub(crate) fn filter(&self, surface: Rect) -> DamagePaint {
        if self.region.is_empty() {
            return DamagePaint::Skip;
        }
        let surface_area = surface.area();
        if surface_area <= 0.0 || self.region.total_area() / surface_area > FULL_REPAINT_THRESHOLD {
            return DamagePaint::Full;
        }
        DamagePaint::Partial(self.region)
    }
}

#[cfg(test)]
mod tests;
