//! Traversal iterators over a [`Tree`](crate::scene::tree::Tree): [`ChildIter`]/[`Child`] (direct children) and [`TreeItems`]/[`TreeItem`] (a node's shapes interleaved with its children in record order), the single source of the shape-cursor logic for encoder, cascade and hash walks.

use soa_rs::Soa;

use crate::common::span::Span;
use crate::primitives::layout::visibility::Visibility;
use crate::scene::node::layout_core::LayoutCore;
use crate::scene::tree::node_id::NodeId;
use crate::scene::tree::node_record::NodeRecord;
use crate::scene::tree::subtree_end::SubtreeEnd;
use crate::shape::record::ShapeRecord;

#[derive(Debug)]
pub(crate) struct ChildIter<'a> {
    layouts: &'a [LayoutCore],
    ends: &'a [SubtreeEnd],
    next: u32,
    end: u32,
}

impl<'a> ChildIter<'a> {
    pub(crate) fn new(records: &'a Soa<NodeRecord>, parent: NodeId) -> Self {
        let ends = records.subtree_end();
        Self {
            layouts: records.layout(),
            next: parent.0 + 1,
            end: ends[parent.idx()].end(),
            ends,
        }
    }
}

#[derive(Copy, Clone, Debug)]
pub(crate) enum TreeItem<'a> {
    /// The `u32` is the shape's index into `Tree::shapes.records`, used by the encoder's paint-animation cursor.
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

impl Iterator for ChildIter<'_> {
    type Item = Child;
    fn next(&mut self) -> Option<Child> {
        if self.next >= self.end {
            return None;
        }
        let i = self.next as usize;
        let visibility = self.layouts[i].meta.visibility();
        self.next = self.ends[i].end();
        Some(Child {
            id: NodeId(i as u32),
            visibility,
        })
    }
}

#[derive(Debug)]
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
            let visibility = self.layouts[self.next_child_id as usize].meta.visibility();
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
