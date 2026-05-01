use crate::element::{Element, ElementCore, ElementExtras, LayoutMode};
use crate::primitives::Track;
use crate::shape::Shape;
use std::rc::Rc;

mod grid_def;
pub(crate) use grid_def::GridDef;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct NodeId(pub(crate) u32);

impl NodeId {
    pub fn index(self) -> usize {
        self.0 as usize
    }
}

/// Recorded node — the user-described data for one element. Layout output
/// (`desired`, `rect`) is *not* on `Node`; it lives on `LayoutEngine` keyed
/// by `NodeId`. `Tree` is therefore read-only after recording finishes.
///
/// Topology is encoded by `subtree_end`: an exclusive index into `Tree.nodes`
/// one past the last descendant of this node. Since `nodes` is stored in
/// pre-order, this node's children start at `self_id + 1` and the whole
/// subtree is `nodes[self_id..subtree_end]`. To iterate direct children, jump
/// from `self_id + 1` by each child's own `subtree_end` until reaching the
/// parent's. No `parent` / `first_child` / `next_sibling` links needed.
#[derive(Debug)]
pub struct Node {
    /// What was recorded for this node: id, layout, mode, sense, disabled.
    /// Effective `Sense::NONE` from disabled-cascade is computed in
    /// `InputState::end_frame` by walking ancestors — `element.disabled` is
    /// just the locally-declared bit.
    pub element: ElementCore,

    /// Exclusive index into `Tree.nodes` one past this node's last descendant.
    /// `self_id + 1 == subtree_end` for a leaf or a not-yet-populated parent.
    pub subtree_end: u32,
}

impl Node {
    pub fn is_collapsed(&self) -> bool {
        self.element.attrs.is_collapsed()
    }

    fn new(element: ElementCore, self_id: u32) -> Self {
        Self {
            element,
            subtree_end: self_id + 1,
        }
    }
}

/// `nodes` are stored in pre-order paint order: a parent is pushed before its
/// children, and siblings appear in declaration order. Reverse iteration gives
/// topmost-first traversal — load-bearing for hit-testing in `Ui`.
pub struct Tree {
    pub(crate) nodes: Vec<Node>,
    pub(crate) shapes: Vec<Shape>,
    /// Per-node shape-range starts, length always `nodes.len() + 1`. The
    /// shapes for node `i` are `shapes[shape_starts[i]..shape_starts[i+1]]`;
    /// the trailing sentinel is the open end of the last node, kept equal to
    /// `shapes.len()` while recording so `add_shape` can extend it in place.
    shape_starts: Vec<u32>,
    /// Recording-only scratch: index `i` holds the parent of node `i` (or
    /// `None` if root). Used by `push_node` to walk up the ancestor chain
    /// bumping `subtree_end`. Not read after recording — kept as a parallel
    /// vec rather than a `Node` field so the Node footprint stays minimal
    /// across measure/arrange/paint. Reused frame-to-frame.
    recording_parent: Vec<Option<NodeId>>,
    /// Frame-scoped grid storage: track defs (addressed by
    /// `LayoutMode::Grid(u16)`). Per-track hug arrays live on `LayoutResult`
    /// since the tree is read-only after recording. Cleared per frame,
    /// capacity retained.
    grid: GridArena,
    /// Out-of-line side table for rarely-set element fields (`transform`,
    /// `position`, `grid`). `Node.element.extras` is `Some(idx)` when a node
    /// customized any of these. Cleared per frame.
    pub(crate) node_extras: Vec<ElementExtras>,
}

impl Default for Tree {
    fn default() -> Self {
        Self {
            nodes: Vec::new(),
            shapes: Vec::new(),
            shape_starts: vec![0],
            recording_parent: Vec::new(),
            grid: GridArena::default(),
            node_extras: Vec::new(),
        }
    }
}

impl Tree {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn clear(&mut self) {
        self.nodes.clear();
        self.shapes.clear();
        self.shape_starts.clear();
        self.shape_starts.push(0);
        self.recording_parent.clear();
        self.grid.clear();
        self.node_extras.clear();
    }

    pub(crate) fn push_grid_def(
        &mut self,
        rows: Rc<[Track]>,
        cols: Rc<[Track]>,
        row_gap: f32,
        col_gap: f32,
    ) -> u16 {
        self.grid.push_def(rows, cols, row_gap, col_gap)
    }

    pub(crate) fn grid_def(&self, idx: u16) -> &GridDef {
        self.grid.def(idx)
    }

    pub(crate) fn grid_defs(&self) -> &[GridDef] {
        &self.grid.defs
    }

    pub fn push_node(&mut self, element: Element, parent: Option<NodeId>) -> NodeId {
        let new_id = NodeId(self.nodes.len() as u32);
        if let LayoutMode::Grid(idx) = element.mode {
            assert!(
                (idx as usize) < self.grid.defs.len(),
                "LayoutMode::Grid({idx}) references no grid_def — only Grid::show should push grid nodes",
            );
        }
        let (mut core, extras) = element.split();
        if !extras.is_default() {
            assert!(
                self.node_extras.len() < u16::MAX as usize,
                "more than 65 535 nodes with extras (transform/position/grid) in a single frame",
            );
            let idx = self.node_extras.len() as u16;
            self.node_extras.push(extras);
            core.extras = Some(idx);
        }
        self.nodes.push(Node::new(core, new_id.0));
        self.shape_starts.push(self.shapes.len() as u32);
        self.recording_parent.push(parent);

        // Walk up the ancestor chain, growing each one's `subtree_end` so the
        // new node falls inside every ancestor's subtree. Cheap in practice:
        // typical UI trees are shallow.
        let new_end = new_id.0 + 1;
        let mut anc = parent;
        while let Some(a) = anc {
            let ai = a.0 as usize;
            self.nodes[ai].subtree_end = new_end;
            anc = self.recording_parent[ai];
        }
        new_id
    }

    pub fn add_shape(&mut self, node: NodeId, shape: Shape) {
        let idx = node.0 as usize;
        assert_eq!(
            idx,
            self.nodes.len() - 1,
            "shapes for node {idx} must be added contiguously, before any child node",
        );
        self.shapes.push(shape);
        *self.shape_starts.last_mut().unwrap() = self.shapes.len() as u32;
    }

    pub fn node(&self, id: NodeId) -> &Node {
        &self.nodes[id.0 as usize]
    }

    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    /// Iterate nodes in storage order (== pre-order). The index of each node
    /// equals its `NodeId.0`.
    pub fn nodes_iter(&self) -> std::slice::Iter<'_, Node> {
        self.nodes.iter()
    }

    /// Side-table extras for a node, or `None` if the node didn't customize
    /// any of the rarely-set fields (`transform`, `position`, `grid`).
    pub fn extras(&self, id: NodeId) -> Option<&ElementExtras> {
        self.nodes[id.0 as usize]
            .element
            .extras
            .map(|i| &self.node_extras[i as usize])
    }

    /// Read extras for a node, returning a borrow of `ElementExtras::DEFAULT`
    /// when the node has no side-table entry. Use this when you want to read
    /// individual fields (`gap`, `child_align`, `position`, …) without
    /// duplicating defaults at every call site.
    pub fn read_extras(&self, id: NodeId) -> &ElementExtras {
        self.extras(id).unwrap_or(&ElementExtras::DEFAULT)
    }

    /// First node in pre-order paint order, or `None` if the tree is empty.
    /// Stable while the tree is alive: the root is always `NodeId(0)` once
    /// pushed.
    pub fn root(&self) -> Option<NodeId> {
        if self.nodes.is_empty() {
            None
        } else {
            Some(NodeId(0))
        }
    }

    pub fn shapes_of(&self, id: NodeId) -> &[Shape] {
        let i = id.index();
        let s = self.shape_starts[i] as usize;
        let e = self.shape_starts[i + 1] as usize;
        &self.shapes[s..e]
    }

    /// Iterate child NodeIds of `parent` in declaration order.
    pub fn children(&self, parent: NodeId) -> ChildIter<'_> {
        let pi = parent.0 as usize;
        ChildIter {
            tree: self,
            next: parent.0 + 1,
            end: self.nodes[pi].subtree_end,
        }
    }

    /// Lending-style cursor over a node's children. Unlike `children()`, this
    /// doesn't borrow the tree across iterations, so the caller is free to
    /// recurse into `&mut Tree` between steps (e.g. measure/arrange passes).
    pub fn child_cursor(&self, parent: NodeId) -> ChildCursor {
        let pi = parent.0 as usize;
        ChildCursor {
            next: parent.0 + 1,
            end: self.nodes[pi].subtree_end,
        }
    }
}

pub struct ChildIter<'a> {
    tree: &'a Tree,
    next: u32,
    end: u32,
}

impl<'a> Iterator for ChildIter<'a> {
    type Item = NodeId;
    fn next(&mut self) -> Option<NodeId> {
        if self.next >= self.end {
            return None;
        }
        let cur = NodeId(self.next);
        self.next = self.tree.nodes[self.next as usize].subtree_end;
        Some(cur)
    }
}

/// Lending cursor over a node's children. Holds only the next-pointer so the
/// caller can take `&mut Tree` between steps (recursive measure/arrange).
///
/// Contract: tree topology (`subtree_end`) must not change between `next()`
/// calls. The cursor reads the current child's `subtree_end` to advance, so
/// mutating that field, reparenting children, or appending new descendants
/// to a yielded child afterwards will silently desync iteration. Measure
/// and arrange only mutate `desired` and `rect`, so they're safe.
#[derive(Clone, Copy)]
pub struct ChildCursor {
    next: u32,
    end: u32,
}

impl ChildCursor {
    /// Return the current child and advance. Jumps past the child's whole
    /// subtree using its `subtree_end`.
    pub fn next(&mut self, tree: &Tree) -> Option<NodeId> {
        if self.next >= self.end {
            return None;
        }
        let cur = NodeId(self.next);
        self.next = tree.nodes[self.next as usize].subtree_end;
        Some(cur)
    }
}

/// Frame-scoped recording-only grid storage: track defs (one per `Grid`
/// panel), addressed by `LayoutMode::Grid(u16)`. Per-track hug arrays live
/// on `LayoutResult` since the tree is read-only after recording. Capacity
/// is retained across frames; data is cleared per frame.
#[derive(Default)]
pub(crate) struct GridArena {
    pub(super) defs: Vec<GridDef>,
}

impl GridArena {
    fn clear(&mut self) {
        self.defs.clear();
    }

    /// Append a `GridDef` referencing user-owned `Rc<[Track]>` rows + cols;
    /// return its index. The index is stamped into a `LayoutMode::Grid(idx)`
    /// on the owning panel's `Element`. `Rc` clones are refcount-only.
    fn push_def(
        &mut self,
        rows: Rc<[Track]>,
        cols: Rc<[Track]>,
        row_gap: f32,
        col_gap: f32,
    ) -> u16 {
        assert!(
            self.defs.len() < u16::MAX as usize,
            "more than 65 535 Grid panels in a single frame",
        );
        let idx = self.defs.len() as u16;
        self.defs.push(GridDef {
            rows,
            cols,
            row_gap,
            col_gap,
        });
        idx
    }

    fn def(&self, idx: u16) -> &GridDef {
        &self.defs[idx as usize]
    }
}

#[cfg(test)]
mod tests;
