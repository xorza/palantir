//! Per-frame damage detection. Stage 3 of the damage-rendering plan
//! (see `docs/damage-rendering.md`). Computed in [`Ui::end_frame`]
//! after `compute_hashes`; rebuilds the prev-frame snapshot in the
//! same pass so the diff reads the old entry and writes the new one
//! per node.
//!
//! A node is **dirty** if its `(rect, authoring-hash)` differs from
//! the entry keyed by the same `WidgetId` in `Damage.prev`, OR it
//! had no entry (added). A `WidgetId` present in `Damage.prev` with
//! no matching node this frame contributes its prev rect to damage
//! (removed). The damage rect is the union of every contribution.
//!
//! `Damage.dirty` is the per-node dirty list (added / hash-changed /
//! rect-changed). Currently consumed only by tests; reserved for the
//! identity-based reuse work in `docs/damage-rendering.md`
//! ("Wanted: per-node `RenderCmd` cache, text-shape cache,
//! multi-rect damage, incremental hit-index, debug overlay").

use crate::cascade::CascadeResult;
use crate::primitives::{rect::Rect, widget_id::WidgetId};
use crate::tree::{NodeId, Tree, hash::NodeHash};
use rustc_hash::FxHashMap;

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
/// pre-order paint order. `rect` is the smallest rect enclosing all
/// dirty contributions plus every removed widget's prev rect.
/// `None` when no node is dirty — legitimate when the host called
/// `request_repaint()` but nothing actually changed (e.g., an
/// animation tick that didn't advance any visible state).
///
/// `prev` is the per-`WidgetId` snapshot map carried over from last
/// frame; it's mutated in place during `compute` (read old, write
/// new) so steady-state frames don't allocate.
///
/// Capacities on `dirty` and `prev` are retained across frames.
#[derive(Default)]
pub(crate) struct Damage {
    pub(crate) dirty: Vec<NodeId>,
    pub(crate) rect: Option<Rect>,
    /// Last frame's per-widget `(rect, hash)` snapshot. Read by the
    /// diff in `compute`, then rolled forward in the same pass.
    pub(crate) prev: FxHashMap<WidgetId, NodeSnapshot>,
}

/// Damage-area ratio above which the renderer should skip the
/// per-node filter and clear-redraw the whole surface. Below this,
/// the bookkeeping (scissor + LoadOp::Load + backbuffer copy) wins;
/// above it, the savings are eaten by the overhead. 0.5 matches
/// LVGL's `LV_INV_BUF_SIZE` heuristic.
pub(crate) const FULL_REPAINT_THRESHOLD: f32 = 0.5;

impl Damage {
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
        tree: &Tree,
        cascades: &CascadeResult,
        removed: &[WidgetId],
        surface: Rect,
    ) -> Option<Rect> {
        self.dirty.clear();
        let mut acc: Option<Rect> = None;

        let cascade_rows = &cascades.rows;
        let n = tree.node_count();
        let widget_ids = &tree.widget_ids;
        for i in 0..n {
            let wid = widget_ids[i];
            let curr_rect = cascade_rows[i].screen_rect;
            let curr_hash = tree.hashes[i];
            let curr = NodeSnapshot {
                rect: curr_rect,
                hash: curr_hash,
            };

            let dirty = match self.prev.insert(wid, curr) {
                None => {
                    extend(&mut acc, curr_rect);
                    true
                }
                Some(snap) if snap.hash == curr_hash && snap.rect == curr_rect => false,
                Some(snap) => {
                    extend(&mut acc, snap.rect);
                    extend(&mut acc, curr_rect);
                    true
                }
            };
            if dirty {
                self.dirty.push(NodeId(i as u32));
            }
        }

        // Evict last-frame snapshots for removed widgets; their rect
        // contributes to damage so the area they vacated repaints.
        for wid in removed {
            if let Some(snap) = self.prev.remove(wid) {
                extend(&mut acc, snap.rect);
            }
        }

        self.rect = acc;
        self.filter(surface)
    }

    /// Apply the full-repaint threshold to `self.rect`. Private —
    /// callers should use the value returned from [`Self::compute`].
    /// Tests reach in via the same `pub(crate)` visibility as the
    /// rest of `Damage`'s internals.
    pub(crate) fn filter(&self, surface: Rect) -> Option<Rect> {
        let r = self.rect?;
        let surface_area = surface.area();
        if surface_area <= 0.0 || r.area() / surface_area > FULL_REPAINT_THRESHOLD {
            return None;
        }
        Some(r)
    }
}

#[inline]
fn extend(acc: &mut Option<Rect>, r: Rect) {
    *acc = Some(match *acc {
        None => r,
        Some(a) => a.union(r),
    });
}

#[cfg(test)]
mod tests;
