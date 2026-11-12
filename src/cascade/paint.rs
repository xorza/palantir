//! The per-paint-row arena: one [`Paint`] per pixel-producing contribution,
//! plus the per-node index into it. Split from the rest of the cascade
//! because only damage's per-shape legs and the paint-animation wake read it.

use crate::common::block_arena::BlockArena;
use crate::common::content_hash::ContentHash;
use crate::common::span::Span;
use crate::primitives::geometry::rect::Rect;
use crate::scene::tree::paint_animations::PaintAnimationEntry;

/// One row of a node's paint span: chrome (row 0), one direct shape, or a
/// child marker, in record order. Child markers put shape/child interleave
/// into the span so damage sees z-order changes as row reorders.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Paint {
    /// Screen-space rect after the transform the row paints under and its clip.
    /// Child markers carry `Rect::ZERO`.
    pub(crate) screen: Rect,
    /// Authoring hash: the chrome row hash, the shape hash with its panel's
    /// own transform, or for a child marker the child's `WidgetId` bits.
    pub(crate) hash: ContentHash,
}

/// The damage arena, not `PaintArena::rows`. Exact fit: paint spans are a handful of rows, and a
/// larger granule measurably cost cache density on `damage/workload/shape_churn_*`.
pub(crate) type PaintBlocks = BlockArena<Paint, 1>;

/// Per-plane paint state: the [`Paint`] arena plus a per-node index into it.
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

    /// The index into [`Self::rows`] of the row `entry`'s shape painted, or `None` where a clip
    /// or the `surface` edge hides all of it.
    #[inline]
    pub(crate) fn shown_animation_row(
        &self,
        entry: &PaintAnimationEntry,
        surface: Rect,
    ) -> Option<u32> {
        let span = self.node_spans[entry.node.idx()];
        // `entry.row` came from the recording counter and `compute_paint_rect` emits rows in the same order, so a miss means they drifted.
        debug_assert!(
            entry.row < span.len,
            "paint-anim row {} out of the owner's {} paint rows",
            entry.row,
            span.len,
        );
        let row = span.start + entry.row;
        let shown = !self.rows[row as usize]
            .screen
            .clamp_to(surface)
            .is_paint_empty();
        shown.then_some(row)
    }

    /// Resets both columns for a new frame; `rows` is only seeded with
    /// `node_count`, as capacity converges after warmup.
    pub(super) fn reset_for(&mut self, node_count: usize) {
        self.rows.clear();
        self.rows.reserve(node_count);
        self.node_spans.resize(node_count, Span::default());
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
