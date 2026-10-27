//! The per-node columns one measure snapshot holds.

use crate::common::span::Span;
use crate::layout::intrinsic::len_req::SLOT_COUNT;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::geometry::size::Size;
use glam::Vec2;

/// Every captured tree's per-node columns, appended in pre-order and layer
/// order; an [`ArenaSnapshot`](crate::layout::cache::measure_snapshot::ArenaSnapshot)
/// names its subtree's rows by span.
#[derive(Debug, Default)]
pub(super) struct NodeArenas {
    pub(super) desired: Vec<Size>,
    pub(super) floor: Vec<Size>,
    pub(super) stable_from: Vec<Size>,
    /// Arranged rect per node; `LayoutPass::arrange` replays it instead of
    /// re-running drivers when a subtree's slot is unchanged or translated.
    pub(super) rect: Vec<Rect>,
    /// Slot origin in the parent's inner box; a translated replay rebuilds `rect` from it.
    pub(super) local: Vec<Vec2>,
    pub(super) scroll_content: Vec<Size>,
    pub(super) text_spans: Vec<Span>,
    pub(super) intrinsics: Vec<[f32; SLOT_COUNT]>,
    pub(super) available: Vec<Size>,
}

impl NodeArenas {
    /// Destructured so a new column cannot be forgotten: `node_base` comes
    /// from `desired.len()`, and a longer leftover arena would offset every
    /// later slice of that column.
    pub(super) fn clear(&mut self) {
        let Self {
            desired,
            floor,
            stable_from,
            rect,
            local,
            scroll_content,
            text_spans,
            intrinsics,
            available,
        } = self;
        desired.clear();
        floor.clear();
        stable_from.clear();
        rect.clear();
        local.clear();
        scroll_content.clear();
        text_spans.clear();
        intrinsics.clear();
        available.clear();
    }
}
