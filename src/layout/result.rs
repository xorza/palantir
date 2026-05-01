use crate::primitives::{Rect, Size};
use crate::text::TextCacheKey;
use crate::tree::{NodeId, Tree};
use std::collections::HashMap;

/// Per-frame layout *output*. Read after the layout pass by the encoder, hit
/// index, and any other consumer. Per-frame *scratch* (grid track hugs, etc.)
/// lives on `LayoutContext` instead. Capacity is reused across frames via
/// `resize_for`.
#[derive(Default)]
pub struct LayoutResult {
    desired: Vec<Size>,
    rect: Vec<Rect>,
    /// Per-node reshape result for `Shape::Text` runs whose `wrap` is
    /// `TextWrap::Wrap` and whose committed width during measure is narrower
    /// than the natural unbroken line. The encoder consults this when
    /// emitting text so the renderer uses the wrapped buffer instead of the
    /// recorded one — i.e. it crosses the layout/render boundary, which is
    /// what makes it output rather than scratch. Keyed by `NodeId` because
    /// the wrapping `Text` widget pushes exactly one `Shape::Text` per node.
    text_reshapes: HashMap<NodeId, ReshapedText>,
}

/// Side-table override for one `Shape::Text` whose committed width forced
/// a reshape during measure. Tree's recorded shape stays untouched; the
/// encoder reads `key` from here when present so the renderer picks up the
/// wrapped buffer.
#[derive(Clone, Copy, Debug)]
pub struct ReshapedText {
    pub measured: Size,
    pub key: TextCacheKey,
    pub max_width_px: f32,
}

impl LayoutResult {
    pub(super) fn resize_for(&mut self, tree: &Tree) {
        let n = tree.node_count();
        self.desired.clear();
        self.desired.resize(n, Size::ZERO);
        self.rect.clear();
        self.rect.resize(n, Rect::ZERO);
        self.text_reshapes.clear();
    }

    pub fn desired(&self, id: NodeId) -> Size {
        self.desired[id.index()]
    }

    pub fn rect(&self, id: NodeId) -> Rect {
        self.rect[id.index()]
    }

    pub(super) fn set_desired(&mut self, id: NodeId, v: Size) {
        self.desired[id.index()] = v;
    }

    pub(super) fn set_rect(&mut self, id: NodeId, v: Rect) {
        self.rect[id.index()] = v;
    }

    pub fn text_reshape(&self, id: NodeId) -> Option<&ReshapedText> {
        self.text_reshapes.get(&id)
    }

    pub(super) fn set_text_reshape(&mut self, id: NodeId, r: ReshapedText) {
        self.text_reshapes.insert(id, r);
    }
}
