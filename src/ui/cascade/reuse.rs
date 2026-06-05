//! Cross-frame reuse (O5 stage 1): the per-node gate snapshot, the
//! structure-stability check that licenses NodeId-indexed reuse, and the
//! bulk copy that replays an unchanged subtree's cascade output instead
//! of recomputing it. The walk (`walk.rs`) consults these on its skip
//! gate; everything here is the "don't recompute what didn't change"
//! machinery, kept apart from the recompute itself.

use super::{Cascades, EntryRow};
use crate::forest::Forest;
use crate::forest::Layer;
use crate::forest::per_layer::PerLayer;
use crate::forest::rollups::NodeHash;
use crate::primitives::rect::Rect;
use crate::primitives::span::Span;
use crate::primitives::widget_id::WidgetId;

/// Per-node reuse-gate inputs, snapshotted each frame so the next frame
/// can decide — per subtree — whether the cascade output is unchanged.
/// NodeId-indexed (parallel to `Tree::records`), one `Vec` per layer on
/// `CascadesEngine::prev_snap`. Self-contained: holds every datum the
/// gate and the structure check read, so they touch one array.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct CascadeSnapshot {
    /// `tree.rollups.node[i]` — own authoring. A match means this node's
    /// own transform / clip / disabled / visibility / shapes / chrome
    /// are unchanged, so it hands identical inherited context to its
    /// children even when a deeper descendant changed.
    pub(crate) node_hash: NodeHash,
    /// `tree.rollups.subtree[i]` — authoring of the whole subtree. A
    /// match, *with* unchanged inherited context and origin, means the
    /// entire subtree's cascade output is identical and can be copied.
    pub(crate) subtree_hash: NodeHash,
    /// `layout.rect[i]` — arranged rect. Origin is an arrange *output*,
    /// not folded into `subtree_hash`, so a Fill-sibling reflow can move
    /// a node whose authoring is unchanged; the gate must compare it.
    pub(crate) rect: Rect,
    /// `tree.records.widget_id()[i]` — identity at this NodeId. Compared
    /// across frames to confirm the NodeId → widget mapping is stable;
    /// if not, NodeId-indexed reuse is invalid and the frame falls back
    /// to a full recompute.
    pub(crate) widget_id: WidgetId,
}

/// Previous frame's reuse data for one layer, handed to the walk.
#[derive(Clone, Copy)]
pub(crate) struct PrevTree<'a> {
    pub(crate) cascades: &'a Cascades,
    pub(crate) snap: &'a [CascadeSnapshot],
}

/// True when every layer's NodeId → `WidgetId` mapping is identical to
/// the snapshot — the precondition for NodeId-indexed cross-frame reuse.
/// A changed node count or any shifted id means the prev arrays no
/// longer line up, so the caller must recompute fully.
pub(crate) fn structure_matches(
    forest: &Forest,
    prev_snap: &PerLayer<Vec<CascadeSnapshot>>,
) -> bool {
    for (layer, tree) in forest.iter_paint_order() {
        let snap = &prev_snap[layer];
        let wids = tree.records.widget_id();
        if snap.len() != wids.len() {
            return false;
        }
        if snap.iter().zip(wids).any(|(s, &w)| s.widget_id != w) {
            return false;
        }
    }
    true
}

/// Bulk-copy the cascade output for the subtree `[start, end)` from the
/// previous frame into `out`. Every column the recompute path would
/// produce for these nodes is byte-identical to last frame (the skip
/// gate guarantees it), so it's memcpy'd rather than recomputed.
pub(crate) fn copy_subtree(
    prev: PrevTree<'_>,
    out: &mut Cascades,
    snap_out: &mut Vec<CascadeSnapshot>,
    layer: Layer,
    start: usize,
    end: usize,
) {
    // The subtree's gate snapshot is unchanged too (same authoring +
    // ctx + rect ⇒ same `node_hash`/`subtree_hash`/`rect`/`widget_id`),
    // so it carries over verbatim from last frame.
    snap_out.extend_from_slice(&prev.snap[start..end]);
    let pl = &prev.cascades.layers[layer];
    {
        let cl = &mut out.layers[layer];
        cl.cascade_inputs
            .extend_from_slice(&pl.cascade_inputs[start..end]);
        cl.subtree_paint_rects
            .extend_from_slice(&pl.subtree_paint_rects[start..end]);
        // Paint rows are packed in pre-order, so an earlier changed
        // sibling can shift this subtree's base offset. Copy the rows,
        // then rebase each node's span by the prev→new offset delta. The
        // subtree's rows are contiguous in `[start, end)` pre-order:
        // from node `start`'s span start to node `end`'s (the first node
        // past the subtree), or the row tail when the subtree ends the
        // tree.
        let node_count = pl.paint_arena.node_spans.len();
        let src_start = pl.paint_arena.node_spans[start].start as usize;
        let src_end = if end < node_count {
            pl.paint_arena.node_spans[end].start as usize
        } else {
            pl.paint_arena.rows.len()
        };
        let delta = cl.paint_arena.rows.len() as i64 - src_start as i64;
        cl.paint_arena
            .rows
            .extend_from_slice(&pl.paint_arena.rows[src_start..src_end]);
        for j in start..end {
            let s = pl.paint_arena.node_spans[j];
            cl.paint_arena.node_spans[j] = Span::new((s.start as i64 + delta) as u32, s.len);
        }
    }
    // Hit entries are one global Soa across layers; copy this subtree's
    // rows from the prev frame at the same per-layer base (NodeId stable
    // ⇒ same `entries_base` ⇒ same index).
    let base = pl.entries_base as usize;
    let pe = &prev.cascades.entries;
    for j in start..end {
        let k = base + j;
        out.push_entry(EntryRow {
            widget_id: pe.widget_id()[k],
            rect: pe.rect()[k],
            sense: pe.sense()[k],
            focusable: pe.focusable()[k],
            disabled: pe.disabled()[k],
            layout_rect: pe.layout_rect()[k],
        });
    }
}
