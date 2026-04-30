use crate::geom::Style;
use crate::shape::Shape;
use crate::tree::{LayoutKind, NodeId, Tree, WidgetId};

/// Recorder. Holds a parent stack so begin/end nest correctly.
pub struct Ui {
    pub tree: Tree,
    parents: Vec<NodeId>,
    root: Option<NodeId>,
}

impl Default for Ui {
    fn default() -> Self { Self::new() }
}

impl Ui {
    pub fn new() -> Self {
        Self { tree: Tree::new(), parents: Vec::new(), root: None }
    }

    pub fn begin_frame(&mut self) {
        self.tree.clear();
        self.parents.clear();
        self.root = None;
    }

    pub fn root(&self) -> NodeId {
        self.root.expect("no root pushed yet — call begin_node before any other ops")
    }

    pub fn begin_node(&mut self, id: WidgetId, style: Style, layout: LayoutKind) -> NodeId {
        let parent = self.parents.last().copied();
        let node = self.tree.push_node(id, style, layout, parent);
        if self.root.is_none() { self.root = Some(node); }
        self.parents.push(node);
        node
    }

    pub fn end_node(&mut self, node: NodeId) {
        let popped = self.parents.pop().expect("end_node without matching begin_node");
        debug_assert_eq!(popped, node, "end_node called on a non-current node");
    }

    /// Add a shape to `node`. Must be called *before* any child of `node` is begun.
    pub fn add_shape(&mut self, node: NodeId, shape: Shape) {
        self.tree.add_shape(node, shape);
    }

    /// Container helper. Closure runs between begin/end.
    pub fn container<R>(
        &mut self,
        id: WidgetId,
        style: Style,
        layout: LayoutKind,
        f: impl FnOnce(&mut Ui) -> R,
    ) -> R {
        let node = self.begin_node(id, style, layout);
        let r = f(self);
        self.end_node(node);
        r
    }
}
