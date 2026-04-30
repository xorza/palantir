use crate::element::UiElement;
use crate::primitives::{Rect, Size};
use crate::shape::Shape;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct NodeId(pub u32);

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum LayoutMode {
    Leaf,
    HStack,
    VStack,
    /// Children all laid out at the same position (top-left of inner rect),
    /// each sized per its own `Sizing`. Used by `Panel`.
    ZStack,
    /// Children placed at their declared `Layout.position` (parent-inner coords).
    /// Each child sized per its desired (intrinsic) size. Canvas hugs to the
    /// bounding box of placed children.
    Canvas,
}

#[derive(Debug)]
pub struct Node {
    /// What was recorded for this node: id, layout, mode, sense, disabled.
    /// Effective `Sense::NONE` from disabled-cascade is computed in
    /// `InputState::end_frame` by walking ancestors — `element.disabled` is
    /// just the locally-declared bit.
    pub element: UiElement,

    pub parent: Option<NodeId>,
    pub first_child: Option<NodeId>,
    pub last_child: Option<NodeId>,
    pub next_sibling: Option<NodeId>,

    /// Range into Tree.shapes
    pub shapes_start: u32,
    pub shapes_end: u32,

    pub desired: Size,
    pub rect: Rect,
}

impl Node {
    fn new(element: UiElement, parent: Option<NodeId>) -> Self {
        Self {
            element,
            parent,
            first_child: None,
            last_child: None,
            next_sibling: None,
            shapes_start: 0,
            shapes_end: 0,
            desired: Size::ZERO,
            rect: Rect::ZERO,
        }
    }
}

/// `nodes` are stored in pre-order paint order: a parent is pushed before its
/// children, and siblings appear in declaration order. Reverse iteration gives
/// topmost-first traversal — load-bearing for hit-testing in `Ui`.
#[derive(Default)]
pub struct Tree {
    pub nodes: Vec<Node>,
    pub shapes: Vec<Shape>,
}

impl Tree {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn clear(&mut self) {
        self.nodes.clear();
        self.shapes.clear();
    }

    pub fn push_node(&mut self, element: UiElement, parent: Option<NodeId>) -> NodeId {
        let new_id = NodeId(self.nodes.len() as u32);
        let mut node = Node::new(element, parent);
        node.shapes_start = self.shapes.len() as u32;
        node.shapes_end = self.shapes.len() as u32;
        self.nodes.push(node);

        if let Some(p) = parent {
            // Append as last sibling.
            let parent_last = self.nodes[p.0 as usize].last_child;
            match parent_last {
                None => {
                    self.nodes[p.0 as usize].first_child = Some(new_id);
                }
                Some(prev) => {
                    self.nodes[prev.0 as usize].next_sibling = Some(new_id);
                }
            }
            self.nodes[p.0 as usize].last_child = Some(new_id);
        }
        new_id
    }

    pub fn add_shape(&mut self, node: NodeId, shape: Shape) {
        let idx = node.0 as usize;
        debug_assert_eq!(
            self.nodes[idx].shapes_end,
            self.shapes.len() as u32,
            "shapes for node {idx} must be added contiguously, before any child node",
        );
        self.shapes.push(shape);
        self.nodes[idx].shapes_end = self.shapes.len() as u32;
    }

    pub fn node(&self, id: NodeId) -> &Node {
        &self.nodes[id.0 as usize]
    }
    pub fn node_mut(&mut self, id: NodeId) -> &mut Node {
        &mut self.nodes[id.0 as usize]
    }

    pub fn shapes_of(&self, id: NodeId) -> &[Shape] {
        let n = self.node(id);
        &self.shapes[n.shapes_start as usize..n.shapes_end as usize]
    }

    /// Iterate child NodeIds of `parent` in declaration order.
    pub fn children(&self, parent: NodeId) -> ChildIter<'_> {
        ChildIter {
            tree: self,
            next: self.nodes[parent.0 as usize].first_child,
        }
    }
}

pub struct ChildIter<'a> {
    tree: &'a Tree,
    next: Option<NodeId>,
}

impl<'a> Iterator for ChildIter<'a> {
    type Item = NodeId;
    fn next(&mut self) -> Option<NodeId> {
        let cur = self.next?;
        self.next = self.tree.nodes[cur.0 as usize].next_sibling;
        Some(cur)
    }
}

#[cfg(test)]
mod tests;
