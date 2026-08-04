//! The per-paint-row arena: one [`Paint`] per pixel-producing
//! contribution, plus the per-node index into it.
//!
//! Split out from the cascade product because these rows are read on a
//! different schedule from the rest of it — only damage's per-shape legs
//! touch them, behind a `node_spans[i]` indirection its subtree-skip
//! fast path never follows.

use crate::common::content_hash::ContentHash;
use crate::primitives::rect::Rect;
use crate::primitives::span::Span;
use crate::scene::tree::record::{NodeId, SubtreeEnd};

/// One row of a node's paint span — chrome (row 0 when the node has
/// chrome), one direct shape, or a child marker, in record order.
/// Single source of truth for "did this pixel-producer change since
/// last frame?" — including paint *order*: child markers put the
/// shape/child interleave into the span, so the damage diff's row
/// matcher sees z-order changes (a raised node, a shape crossing a
/// child boundary) as row reorders, not silent no-ops.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Paint {
    /// Screen-space rect after parent transform + clip. Child markers
    /// carry `Rect::ZERO` — they produce no pixels themselves (the
    /// child's own rows do); damage computes a child's painted extent
    /// on demand from its subtree's rows when an order check needs it.
    pub(crate) screen: Rect,
    /// Authoring hash. For chrome: the owner's `ChromeRow::hash`.
    /// For shape: `Tree.shapes.hashes[shape_idx]`. For a child marker:
    /// the child's `WidgetId` bits — its stable identity across
    /// reorders.
    pub(crate) hash: ContentHash,
}

/// Per-layer paint state: the unified [`Paint`] arena plus a per-node
/// index into it. A full cascade rebuild resets it; an incremental
/// pass copies changed spans into retained rows.
#[derive(Debug, Default)]
pub(crate) struct PaintArena {
    /// One [`Paint`] row per chrome contribution (row 0 of a node's
    /// span when present), direct shape, or immediate-child marker,
    /// in record order per node. Pushed in pre-order paint order;
    /// cleared by [`Self::reset_for`].
    pub(crate) rows: Vec<Paint>,
    /// Per-node [`Span`] into [`Self::rows`]. Empty span
    /// (`Span::default()`) means the node paints nothing — replaces
    /// the old `rollups.paints` bitset.
    pub(crate) node_spans: Vec<Span>,
}

impl PaintArena {
    /// Reset both columns for a new frame. `n_nodes` resizes
    /// `node_spans`; every retained slot is overwritten by
    /// [`compute_paint_rect`].
    ///
    /// `rows` is cleared and seeded with `n_nodes` — a rough seed, not a
    /// bound in either direction. The row count tracks chrome rows,
    /// shapes and edges rather than nodes, and an invisible node emits
    /// none at all, so a content-bearing tree lands near `2 * n_nodes`
    /// while a chromeless, shapeless container tree lands at
    /// `n_nodes - 1` (child markers only). Since `rows` is cleared and
    /// never shrunk, capacity converges on the high-water mark after
    /// warmup and the seed stops mattering; the true upper bound
    /// (`chrome_table.len() + shapes.records.len() + n_nodes - roots.len()`)
    /// is not worth threading in for the warmup reallocs alone, and
    /// would over-reserve whenever a large subtree is hidden.
    pub(super) fn reset_for(&mut self, n_nodes: usize) {
        self.rows.clear();
        self.rows.reserve(n_nodes);
        self.node_spans.resize(n_nodes, Span::default());
    }

    /// Screen-space painted extent of `node`'s whole subtree, or `None`
    /// when the subtree paints nothing.
    ///
    /// Folded from the per-row `Paint.screen` rects rather than read off
    /// `LayerCascade::subtree_paint_rects`, so a non-painting descendant
    /// can't bias the answer.
    ///
    /// One linear fold, no per-node span hops: the cascade visits nodes
    /// in pre-order with a monotone row cursor and stamps every node's
    /// slot (an empty span still carries the cursor as its `start`), so a
    /// subtree's rows are one contiguous run — this node's `start`
    /// through the `start` of the first node past the subtree.
    pub(crate) fn subtree_extent(&self, node: NodeId, subtree_end: &[SubtreeEnd]) -> Option<Rect> {
        let end = subtree_end[node.idx()].end() as usize;
        let start_row = self.node_spans[node.idx()].start as usize;
        let end_row = match self.node_spans.get(end) {
            Some(next) => next.start as usize,
            None => self.rows.len(),
        };
        self.rows[start_row..end_row].union_screens()
    }
}

/// Slice-level reads over a run of paint rows.
///
/// Both arenas holding `Paint`s hand out bare slices — the cascade's live
/// [`PaintArena::rows`] and the damage diff's retained
/// `PaintSnapArena::snaps` — so these belong on the slice rather than on
/// either owner, which is what kept them scattered as free functions
/// across two damage files.
pub(crate) trait PaintRows {
    /// The rows that actually produce pixels, in row order. Child markers
    /// and fully clipped-away shapes carry a paint-empty screen and drop
    /// out here.
    fn screens(&self) -> impl Iterator<Item = Rect>;

    /// Screen-space union of [`Self::screens`], or `None` when no row
    /// produces pixels. The empty ones stay out: folding a child
    /// marker's zero box in would drag the union to the origin, and a
    /// clipped-away shape's would drag it to the clip edge.
    fn union_screens(&self) -> Option<Rect>;

    /// Whether any row produces visible pixels on `surface`.
    fn any_on_surface(&self, surface: Rect) -> bool;
}

impl PaintRows for [Paint] {
    #[inline]
    fn screens(&self) -> impl Iterator<Item = Rect> {
        self.iter()
            .map(|paint| paint.screen)
            .filter(|screen| !screen.is_paint_empty())
    }

    #[inline]
    fn union_screens(&self) -> Option<Rect> {
        self.screens().reduce(|acc, screen| acc.union(screen))
    }

    #[inline]
    fn any_on_surface(&self, surface: Rect) -> bool {
        self.screens().any(|screen| screen.intersects(surface))
    }
}
