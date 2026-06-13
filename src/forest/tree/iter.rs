//! Traversal iterators over a [`Tree`](super::Tree): [`ChildIter`] /
//! [`Child`] (direct children, collapse-tagged) and [`TreeItems`] /
//! [`TreeItem`] (a node's direct shapes interleaved with its immediate
//! children in record order). The latter is the single source of truth
//! for the parent/child shape-cursor logic — the encoder, cascade, and
//! hash walks all drive it.

use soa_rs::Soa;

use crate::forest::element::LayoutCore;
use crate::forest::node::{NodeRecord, SubtreeEnd};
use crate::forest::shapes::record::ShapeRecord;
use crate::forest::tree::NodeId;
use crate::forest::visibility::Visibility;
use crate::primitives::span::Span;

pub(crate) struct ChildIter<'a> {
    pub(super) layouts: &'a [LayoutCore],
    pub(super) ends: &'a [SubtreeEnd],
    pub(super) next: u32,
    pub(super) end: u32,
}

#[derive(Copy, Clone, Debug)]
pub(crate) enum TreeItem<'a> {
    /// `u32` is the shape's index into `Tree::shapes.records` — used
    /// by the encoder to look up paint-anim registrations via
    /// `Tree::paint_anims.by_shape[idx]`. Cascade / testing call sites
    /// that only care about the record itself can ignore it.
    ShapeRecord(u32, &'a ShapeRecord),
    Child(Child),
}

#[derive(Copy, Clone, Debug)]
pub(crate) struct Child {
    pub(crate) id: NodeId,
    pub(crate) visibility: Visibility,
}

impl Child {
    #[inline]
    pub(crate) fn active(self) -> Option<NodeId> {
        (!self.visibility.is_collapsed()).then_some(self.id)
    }
}

impl<'a> Iterator for ChildIter<'a> {
    type Item = Child;
    fn next(&mut self) -> Option<Child> {
        if self.next >= self.end {
            return None;
        }
        let i = self.next as usize;
        let visibility = self.layouts[i].visibility();
        self.next = self.ends[i].end();
        Some(Child {
            id: NodeId(i as u32),
            visibility,
        })
    }
}

pub(crate) struct TreeItems<'a> {
    shapes_col: &'a [Span],
    layouts: &'a [LayoutCore],
    ends: &'a [SubtreeEnd],
    shapes: &'a [ShapeRecord],
    cursor: usize,
    parent_end: usize,
    next_child_id: u32,
    subtree_end: u32,
}

impl<'a> TreeItems<'a> {
    pub(crate) fn new(
        records: &'a Soa<NodeRecord>,
        shapes: &'a [ShapeRecord],
        node: NodeId,
    ) -> Self {
        let shapes_col = records.shape_span();
        let parent = shapes_col[node.idx()];
        let ends = records.subtree_end();
        Self {
            shapes_col,
            layouts: records.layout(),
            ends,
            shapes,
            cursor: parent.start as usize,
            parent_end: (parent.start + parent.len) as usize,
            next_child_id: node.0 + 1,
            subtree_end: ends[node.idx()].end(),
        }
    }
}

impl<'a> Iterator for TreeItems<'a> {
    type Item = TreeItem<'a>;
    fn next(&mut self) -> Option<TreeItem<'a>> {
        if self.next_child_id < self.subtree_end {
            let cs = self.shapes_col[self.next_child_id as usize];
            let cs_start = cs.start as usize;
            if self.cursor < cs_start {
                let idx = self.cursor as u32;
                let s = &self.shapes[self.cursor];
                self.cursor += 1;
                return Some(TreeItem::ShapeRecord(idx, s));
            }
            let visibility = self.layouts[self.next_child_id as usize].visibility();
            let child = Child {
                id: NodeId(self.next_child_id),
                visibility,
            };
            self.cursor = cs_start + cs.len as usize;
            self.next_child_id = self.ends[self.next_child_id as usize].end();
            return Some(TreeItem::Child(child));
        }
        if self.cursor < self.parent_end {
            let idx = self.cursor as u32;
            let s = &self.shapes[self.cursor];
            self.cursor += 1;
            return Some(TreeItem::ShapeRecord(idx, s));
        }
        None
    }
}
