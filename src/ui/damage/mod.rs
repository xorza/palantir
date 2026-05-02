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

use crate::cascade::Cascades;
use crate::primitives::{Rect, WidgetId};
use crate::tree::{NodeId, Tree};
use rustc_hash::{FxHashMap, FxHashSet};

/// Per-widget snapshot retained across frames so the next frame's
/// `Damage::compute` can diff `(rect, hash)` against the previous
/// value. Indexed by stable [`WidgetId`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct NodeSnapshot {
    /// Screen-space rect from last frame's `Cascade.screen_rect`.
    pub rect: Rect,
    /// Authoring hash from last frame's `Tree.hashes`.
    pub hash: u64,
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
    pub dirty: Vec<NodeId>,
    pub rect: Option<Rect>,
    /// `true` when the damage rect covers more than
    /// [`FULL_REPAINT_THRESHOLD`] of the surface — the encoder/backend
    /// should skip the per-node filter and clear-redraw everything.
    /// Set by [`Damage::compute`] via [`needs_full_repaint`].
    pub full_repaint: bool,
    /// Last frame's per-widget `(rect, hash)` snapshot. Read by the
    /// diff in `compute`, then rolled forward in the same pass.
    pub prev: FxHashMap<WidgetId, NodeSnapshot>,
}

/// Damage-area ratio above which the renderer should skip the
/// per-node filter and clear-redraw the whole surface. Below this,
/// the bookkeeping (scissor + LoadOp::Load + backbuffer copy) wins;
/// above it, the savings are eaten by the overhead. 0.5 matches
/// LVGL's `LV_INV_BUF_SIZE` heuristic.
pub(crate) const FULL_REPAINT_THRESHOLD: f32 = 0.5;

/// Decide between a partial repaint (scissored to `damage.rect`) and
/// a full-surface repaint. `true` when the damage rect covers more
/// than [`FULL_REPAINT_THRESHOLD`] of the surface — beyond that, the
/// scissor + backbuffer-copy overhead exceeds the per-pixel savings
/// of partial repaint. `false` when damage is small *or* `None`
/// (nothing to do).
///
/// A degenerate zero-area surface short-circuits to full repaint;
/// it shouldn't happen in practice (host filters resize-to-zero),
/// but cheap to handle.
fn needs_full_repaint(damage: &Damage, surface: Rect) -> bool {
    let surface_area = surface.area();
    if surface_area <= 0.0 {
        return true;
    }
    match damage.rect {
        None => false,
        Some(r) => r.area() / surface_area > FULL_REPAINT_THRESHOLD,
    }
}

impl Damage {
    /// Diff against the just-finished frame and roll `self.prev`
    /// forward to this frame's snapshot in the same pass — the diff
    /// reads each `WidgetId`'s old entry via `insert`, then any
    /// surplus entries (removed widgets) are swept after the loop.
    /// `curr_ids` is this frame's widget-id set — reused from
    /// `Ui.seen_ids` so we don't rebuild it. `surface` is the rect
    /// [`Ui::layout`] was called with; used to decide the
    /// partial-vs-full-repaint heuristic.
    ///
    /// Rects are tracked in **screen space** (read straight off
    /// `Cascade.screen_rect`). This makes damage match where the GPU
    /// actually paints, so the backend scissor lands on the right
    /// pixels even under transformed parents.
    pub fn compute(
        &mut self,
        tree: &Tree,
        cascades: &Cascades,
        curr_ids: &FxHashSet<WidgetId>,
        surface: Rect,
    ) {
        self.dirty.clear();
        let mut acc: Option<Rect> = None;

        let cascade_rows = cascades.rows();
        let n = tree.node_count();
        let widget_ids = tree.widget_ids();
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

        // Removed widgets: entries in `prev` whose `wid` wasn't recorded
        // this frame. They contribute their last-known rect to damage
        // and must be evicted from `prev` so next frame's diff doesn't
        // see them. `n` entries were just inserted/refreshed above; if
        // `prev.len() > n`, the surplus is exactly the removed set.
        if self.prev.len() > n {
            self.prev.retain(|wid, snap| {
                if curr_ids.contains(wid) {
                    true
                } else {
                    extend(&mut acc, snap.rect);
                    false
                }
            });
        }

        self.rect = acc;
        self.full_repaint = needs_full_repaint(self, surface);
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
