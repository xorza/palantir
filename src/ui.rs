use crate::primitives::{Style, WidgetId};
use crate::shape::Shape;
use crate::tree::{LayoutKind, NodeId, Tree};
use std::collections::HashMap;

/// Recorder. Holds a parent stack so begin/end nest correctly.
pub struct Ui {
    pub tree: Tree,
    parents: Vec<NodeId>,
    root: Option<NodeId>,
    #[cfg(debug_assertions)]
    seen_ids: HashMap<WidgetId, NodeId>,
}

impl Default for Ui {
    fn default() -> Self { Self::new() }
}

impl Ui {
    pub fn new() -> Self {
        Self {
            tree: Tree::new(),
            parents: Vec::new(),
            root: None,
            #[cfg(debug_assertions)]
            seen_ids: HashMap::new(),
        }
    }

    pub fn begin_frame(&mut self) {
        self.tree.clear();
        self.parents.clear();
        self.root = None;
        #[cfg(debug_assertions)]
        self.seen_ids.clear();
    }

    pub fn root(&self) -> NodeId {
        self.root.expect("no root pushed yet — open a node before any other ops")
    }

    /// Open a node, run `f` to populate its shapes and children, close it.
    pub(crate) fn node(
        &mut self,
        id: WidgetId,
        style: Style,
        layout: LayoutKind,
        f: impl FnOnce(&mut Ui),
    ) -> NodeId {
        let parent = self.parents.last().copied();
        let node = self.tree.push_node(id, style, layout, parent);
        #[cfg(debug_assertions)]
        if let Some(prev) = self.seen_ids.insert(id, node) {
            tracing::warn!(
                ?id, ?node, first_seen = ?prev,
                "WidgetId collision — use `with_id(...)` to disambiguate"
            );
        }
        if self.root.is_none() { self.root = Some(node); }
        self.parents.push(node);
        f(self);
        self.parents.pop();
        node
    }

    /// Append a shape to the currently-open node. Must be called before any child node opens.
    pub(crate) fn add_shape(&mut self, shape: Shape) {
        let node = *self.parents.last().expect("add_shape called outside any open node");
        self.tree.add_shape(node, shape);
    }
}
