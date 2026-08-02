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
    /// Authoring hash. For chrome: `Tree.rollups.chrome[node]`.
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
    /// [`compute_paint_rect`]. `rows` is cleared and reserved for the
    /// expected upper bound.
    pub(super) fn reset_for(&mut self, n_nodes: usize) {
        self.rows.clear();
        self.rows.reserve(n_nodes);
        self.node_spans.resize(n_nodes, Span::default());
    }
}
