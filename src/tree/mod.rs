use crate::primitives::{Layout, Rect, Sense, Size, WidgetId};
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
    pub id: WidgetId,
    pub parent: Option<NodeId>,
    pub first_child: Option<NodeId>,
    pub last_child: Option<NodeId>,
    pub next_sibling: Option<NodeId>,

    pub layout: Layout,
    pub mode: LayoutMode,
    pub sense: Sense,
    /// Suppress this node's interactions and cascade to all descendants.
    /// Set by widgets like `Panel::disabled(true)`. Effective `Sense::NONE`
    /// is computed in `InputState::end_frame` by walking ancestors.
    pub disabled: bool,

    /// Range into Tree.shapes
    pub shapes_start: u32,
    pub shapes_end: u32,

    pub desired: Size,
    pub rect: Rect,
}

impl Node {
    fn new(
        id: WidgetId,
        layout: Layout,
        mode: LayoutMode,
        sense: Sense,
        parent: Option<NodeId>,
    ) -> Self {
        Self {
            id,
            parent,
            first_child: None,
            last_child: None,
            next_sibling: None,
            layout,
            mode,
            sense,
            disabled: false,
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

    pub fn push_node(
        &mut self,
        id: WidgetId,
        layout: Layout,
        mode: LayoutMode,
        sense: Sense,
        parent: Option<NodeId>,
    ) -> NodeId {
        let new_id = NodeId(self.nodes.len() as u32);
        let mut node = Node::new(id, layout, mode, sense, parent);
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

    /// Lending-style cursor over a node's children. Unlike `children()`, this
    /// doesn't borrow the tree across iterations, so the caller is free to
    /// recurse into `&mut Tree` between steps (e.g. measure/arrange passes).
    pub fn child_cursor(&self, parent: NodeId) -> ChildCursor {
        ChildCursor {
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

/// Lending cursor over a node's children. Holds only the next-pointer so the
/// caller can take `&mut Tree` between steps (recursive measure/arrange).
///
/// Contract: tree topology (`first_child` / `next_sibling`) must not change
/// between `next()` calls. The cursor snapshots `c.next_sibling` when it
/// returns `c`; mutating that field, reparenting `c`, or inserting siblings
/// after `c` afterwards will silently desync iteration. Measure and arrange
/// only mutate `desired` and `rect`, so they're safe.
#[derive(Clone, Copy)]
pub struct ChildCursor {
    next: Option<NodeId>,
}

impl ChildCursor {
    /// Return the current child and advance. Reads `next_sibling` before
    /// returning so the caller is free to mutate the tree between calls
    /// (subject to the topology-invariance contract on the struct).
    pub fn next(&mut self, tree: &Tree) -> Option<NodeId> {
        let cur = self.next?;
        self.next = tree.nodes[cur.0 as usize].next_sibling;
        Some(cur)
    }

    /// Whether another child remains without advancing.
    pub fn has_next(&self) -> bool {
        self.next.is_some()
    }
}

#[cfg(test)]
mod tests;
