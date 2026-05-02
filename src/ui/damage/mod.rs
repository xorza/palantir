//! Per-frame damage detection. Step 3 of the damage-rendering plan
//! (see `docs/damage-rendering.md`). Computed in [`Ui::end_frame`]
//! after `compute_hashes` but before `rebuild_prev_frame` — needs the
//! previous frame's snapshot intact for the diff.
//!
//! A node is **dirty** if its `(rect, authoring-hash)` differs from
//! the entry keyed by the same `WidgetId` in `Ui.prev_frame`, OR it
//! had no entry (added). A `WidgetId` present in `prev_frame` with
//! no matching node this frame contributes its prev rect to damage
//! (removed). The damage rect is the union of every contribution.
//!
//! Currently *computed but not consumed*: Step 5 (encoder filter) is
//! the first reader. Step 4 (heuristic fallback) and Step 7
//! (transform cascade) are layered on top later.

use crate::layout::LayoutResult;
use crate::primitives::{Rect, WidgetId};
use crate::tree::{NodeId, Tree};
use rustc_hash::{FxHashMap, FxHashSet};

use super::NodeSnapshot;

/// Output of one frame's damage pass.
///
/// `dirty` lists every added / hash-changed / rect-changed node in
/// pre-order paint order. `rect` is the smallest rect enclosing all
/// dirty contributions plus every removed widget's prev rect.
/// `None` when no node is dirty — legitimate when the host called
/// `request_repaint()` but nothing actually changed (e.g., an
/// animation tick that didn't advance any visible state).
///
/// Capacity on `dirty` is retained across frames; `clear()` resets
/// without freeing.
#[derive(Default)]
pub(crate) struct Damage {
    pub dirty: Vec<NodeId>,
    pub rect: Option<Rect>,
}

impl Damage {
    pub fn clear(&mut self) {
        self.dirty.clear();
        self.rect = None;
    }

    /// Recompute against the just-finished frame. `prev` is last
    /// frame's snapshot map (untouched here — caller rebuilds it
    /// after this). `curr_ids` is this frame's widget-id set —
    /// reused from `Ui.seen_ids` so we don't rebuild it.
    pub fn compute(
        &mut self,
        tree: &Tree,
        result: &LayoutResult,
        prev: &FxHashMap<WidgetId, NodeSnapshot>,
        curr_ids: &FxHashSet<WidgetId>,
    ) {
        self.clear();
        let mut acc: Option<Rect> = None;

        for i in 0..tree.node_count() {
            let id = NodeId(i as u32);
            let wid = tree.widget_ids()[i];
            let curr_rect = result.rect(id);
            let curr_hash = tree.hashes[i];

            let dirty = match prev.get(&wid) {
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
                self.dirty.push(id);
            }
        }

        for (wid, snap) in prev {
            if !curr_ids.contains(wid) {
                extend(&mut acc, snap.rect);
            }
        }

        self.rect = acc;
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
