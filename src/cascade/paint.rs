//! The per-paint-row arena: one [`Paint`] per pixel-producing contribution,
//! plus the per-node index into it. Split from the rest of the cascade
//! because only damage's per-shape legs read it.

use crate::common::block_arena::BlockSlot;
use crate::common::content_hash::ContentHash;
use crate::common::span::Span;
use crate::primitives::geometry::rect::Rect;

/// One row of a node's paint span: chrome (row 0), one direct shape, or a
/// child marker, in record order. Child markers put shape/child interleave
/// into the span so damage sees z-order changes as row reorders.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Paint {
    /// Screen-space rect after parent transform + clip. Child markers carry
    /// `Rect::ZERO`.
    pub(crate) screen: Rect,
    /// Authoring hash: the chrome row hash, the shape hash, or for a child
    /// marker the child's `WidgetId` bits.
    pub(crate) hash: ContentHash,
}

/// The free-list link rides in `hash`; free blocks are never read as rows.
/// This is the damage arena's element, not `PaintArena::rows`.
impl BlockSlot for Paint {
    /// Exact fit: paint spans are a handful of rows, and a larger granule
    /// measurably cost cache density on `damage/workload/shape_churn_*`.
    const GRANULE: u32 = 1;

    fn free_link(next: u32) -> Self {
        Self {
            screen: Rect::ZERO,
            hash: ContentHash(u64::from(next)),
        }
    }

    fn next_free(self) -> u32 {
        self.hash.0 as u32
    }
}

/// Per-layer paint state: the [`Paint`] arena plus a per-node index into it.
#[derive(Debug, Default)]
pub(crate) struct PaintArena {
    /// One row per chrome contribution, direct shape, or child marker, in
    /// pre-order paint order.
    pub(crate) rows: Vec<Paint>,
    /// Per-node [`Span`] into [`Self::rows`]; an empty span means the node
    /// paints nothing.
    pub(crate) node_spans: Vec<Span>,
}

impl PaintArena {
    /// The rows node `i` painted. On the arena so the damage walk can hold the
    /// slice without borrowing itself.
    #[inline]
    pub(crate) fn rows_of(&self, i: usize) -> &[Paint] {
        &self.rows[self.node_spans[i].range()]
    }

    /// Resets both columns for a new frame; `rows` is only seeded with
    /// `n_nodes`, as capacity converges after warmup.
    pub(super) fn reset_for(&mut self, n_nodes: usize) {
        self.rows.clear();
        self.rows.reserve(n_nodes);
        self.node_spans.resize(n_nodes, Span::default());
    }
}

/// Slice-level reads over a run of paint rows, shared by the cascade's
/// [`PaintArena::rows`] and the damage diff's retained paints.
pub(crate) trait PaintRows {
    /// Rows that produce pixels, in row order.
    fn screens(&self) -> impl Iterator<Item = Rect>;

    /// Union of [`Self::screens`], [`Rect::ZERO`] when empty.
    fn union_screens(&self) -> Rect;

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
    fn union_screens(&self) -> Rect {
        self.screens().fold(Rect::ZERO, Rect::union)
    }

    #[inline]
    fn any_on_surface(&self, surface: Rect) -> bool {
        self.screens().any(|screen| screen.intersects(surface))
    }
}
