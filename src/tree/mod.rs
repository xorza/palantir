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
#[derive(Debug)]
pub struct Node {
    /// What was recorded for this node: id, layout, mode, sense, disabled.
    /// Effective `Sense::NONE` from disabled-cascade is computed in
    /// `InputState::end_frame` by walking ancestors — `element.disabled` is
    /// just the locally-declared bit.
    pub element: ElementCore,

    pub parent: Option<NodeId>,
    pub first_child: Option<NodeId>,
    pub next_sibling: Option<NodeId>,
}

impl Node {
    pub fn is_collapsed(&self) -> bool {
        self.element.attrs.is_collapsed()
    }

    fn new(element: ElementCore, parent: Option<NodeId>) -> Self {
        Self {
            element,
            parent,
            first_child: None,
            next_sibling: None,
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
    /// Recording-only scratch: index `i` holds the most recently appended
    /// child of node `i`, used by `push_node` for O(1) sibling-list append.
    /// Not read after recording — kept as a parallel vec rather than a `Node`
    /// field so the Node footprint stays minimal across measure/arrange/paint.
    /// Reused frame-to-frame; cleared with `Tree::clear`.
    last_child: Vec<Option<NodeId>>,
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
            last_child: Vec::new(),
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
        self.last_child.clear();
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
        self.nodes.push(Node::new(core, parent));
        self.shape_starts.push(self.shapes.len() as u32);
        self.last_child.push(None);

        if let Some(p) = parent {
            // Append as last sibling. `last_child[p]` is the previous tail (or
            // `None` if `p` had no children yet).
            let pi = p.0 as usize;
            match self.last_child[pi] {
                None => {
                    self.nodes[pi].first_child = Some(new_id);
                }
                Some(prev) => {
                    self.nodes[prev.0 as usize].next_sibling = Some(new_id);
                }
            }
            self.last_child[pi] = Some(new_id);
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
