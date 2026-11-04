//! The per-node columns one measure snapshot holds.

use crate::common::span::Span;
use crate::layout::measure_columns::MeasureColumns;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::geometry::size::Size;
use glam::Vec2;

/// Every captured tree's per-node columns, appended in pre-order and layer
/// order; an [`ArenaSnapshot`](crate::layout::cache::measure_snapshot::ArenaSnapshot)
/// names its subtree's rows by span.
#[derive(Debug, Default)]
pub(super) struct NodeArenas {
    pub(super) measure: MeasureColumns,
    /// Arranged rect per node; `LayoutPass::arrange` replays it instead of
    /// re-running drivers when a subtree's slot is unchanged or translated.
    pub(super) rect: Vec<Rect>,
    /// Slot origin in the parent's inner box; a translated replay rebuilds `rect` from it.
    pub(super) local: Vec<Vec2>,
    pub(super) scroll_content: Vec<Size>,
    pub(super) text_spans: Vec<Span>,
    /// Each container's paint-only runs in the snapshot's text shapes, shaped after arrange at
    /// its arranged width; `None` for a node whose runs were not shaped, as one under a hidden
    /// ancestor, which a hit below a now visible one would otherwise replay as none. Apart from
    /// `text_spans`, which a hit restores whole: these are replayed one container at a time, by
    /// width.
    pub(super) container_spans: Vec<Option<Span>>,
}

impl NodeArenas {
    /// Destructured so a new column cannot be forgotten: `node_base` comes
    /// from `measure.len()`, and a longer leftover arena would offset every
    /// later slice of that column.
    pub(super) fn clear(&mut self) {
        let Self {
            measure,
            rect,
            local,
            scroll_content,
            text_spans,
            container_spans,
        } = self;
        measure.clear();
        rect.clear();
        local.clear();
        scroll_content.clear();
        text_spans.clear();
        container_spans.clear();
    }
}
