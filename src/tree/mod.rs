use crate::element::{NodeElement, UiElement, UiElementExtras};
use crate::primitives::{GridDef, HugSlice, Rect, Size, Track, Visibility};
use crate::shape::Shape;
use std::rc::Rc;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct NodeId(pub(crate) u32);

#[derive(Debug)]
pub struct Node {
    /// What was recorded for this node: id, layout, mode, sense, disabled.
    /// Effective `Sense::NONE` from disabled-cascade is computed in
    /// `InputState::end_frame` by walking ancestors — `element.disabled` is
    /// just the locally-declared bit.
    pub element: NodeElement,

    pub parent: Option<NodeId>,
    pub first_child: Option<NodeId>,
    pub next_sibling: Option<NodeId>,

    /// Half-open range into `Tree.shapes` for this node's shapes.
    pub shapes: std::ops::Range<u32>,

    pub desired: Size,
    pub rect: Rect,
}

impl Node {
    pub fn shapes_range(&self) -> std::ops::Range<usize> {
        self.shapes.start as usize..self.shapes.end as usize
    }

    pub fn is_collapsed(&self) -> bool {
        self.element.visibility == Visibility::Collapsed
    }

    fn new(element: NodeElement, parent: Option<NodeId>, shapes_start: u32) -> Self {
        Self {
            element,
            parent,
            first_child: None,
            next_sibling: None,
            shapes: shapes_start..shapes_start,
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
    pub(crate) nodes: Vec<Node>,
    pub(crate) shapes: Vec<Shape>,
    /// Recording-only scratch: index `i` holds the most recently appended
    /// child of node `i`, used by `push_node` for O(1) sibling-list append.
    /// Not read after recording — kept as a parallel vec rather than a `Node`
    /// field so the Node footprint stays minimal across measure/arrange/paint.
    /// Reused frame-to-frame; cleared with `Tree::clear`.
    last_child: Vec<Option<NodeId>>,
    /// Grid track definitions, addressed by `LayoutMode::Grid(u16)`. One
    /// entry per `Grid` panel recorded this frame. Track lists live behind
    /// `Rc<[Track]>` so callers can cache and share them across frames; the
    /// framework only refcount-touches. Cleared with `clear`.
    grid_defs: Vec<GridDef>,
    /// Shared pool of per-track hug sizes referenced by `GridDef::row_hugs` /
    /// `col_hugs`. Written by `grid_measure` and read by `arrange_grid` so
    /// arrange doesn't have to re-walk children to recompute Hug-track sizing.
    hug_pool: Vec<f32>,
    /// Out-of-line side table for rarely-set element fields (`transform`,
    /// `position`, `grid`). `Node.element.extras` is `Some(idx)` when a node
    /// customized any of these. Cleared per frame.
    pub(crate) node_extras: Vec<UiElementExtras>,
}

impl Tree {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn clear(&mut self) {
        self.nodes.clear();
        self.shapes.clear();
        self.last_child.clear();
        self.grid_defs.clear();
        self.hug_pool.clear();
        self.node_extras.clear();
    }

    /// Append a `GridDef` referencing user-owned `Rc<[Track]>` rows + cols;
    /// return its index. The index is stamped into a `LayoutMode::Grid(idx)`
    /// on the owning panel's `UiElement`. Reserves zero-initialized hug-size
    /// slots that `grid_measure` will fill in. The `Rc` clones are
    /// refcount-only — no track data is copied.
    pub(crate) fn push_grid_def(
        &mut self,
        rows: Rc<[Track]>,
        cols: Rc<[Track]>,
        row_gap: f32,
        col_gap: f32,
    ) -> u16 {
        let row_hugs = self.reserve_hugs(rows.len());
        let col_hugs = self.reserve_hugs(cols.len());
        assert!(
            self.grid_defs.len() < u16::MAX as usize,
            "more than 65 535 Grid panels in a single frame",
        );
        let idx = self.grid_defs.len() as u16;
        self.grid_defs.push(GridDef {
            rows,
            cols,
            row_gap,
            col_gap,
            row_hugs,
            col_hugs,
        });
        idx
    }

    fn reserve_hugs(&mut self, n: usize) -> HugSlice {
        let start = self.hug_pool.len() as u32;
        self.hug_pool.resize(start as usize + n, 0.0);
        HugSlice {
            start,
            len: n as u32,
        }
    }

    pub(crate) fn grid_def(&self, idx: u16) -> &GridDef {
        &self.grid_defs[idx as usize]
    }

    pub(crate) fn grid_hugs(&self, slice: HugSlice) -> &[f32] {
        &self.hug_pool[slice.range()]
    }

    pub(crate) fn grid_hugs_mut(&mut self, slice: HugSlice) -> &mut [f32] {
        &mut self.hug_pool[slice.range()]
    }

    pub fn push_node(&mut self, element: UiElement, parent: Option<NodeId>) -> NodeId {
        let new_id = NodeId(self.nodes.len() as u32);
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
        self.nodes
            .push(Node::new(core, parent, self.shapes.len() as u32));
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
        debug_assert_eq!(
            self.nodes[idx].shapes.end,
            self.shapes.len() as u32,
            "shapes for node {idx} must be added contiguously, before any child node",
        );
        self.shapes.push(shape);
        self.nodes[idx].shapes.end = self.shapes.len() as u32;
    }

    pub fn node(&self, id: NodeId) -> &Node {
        &self.nodes[id.0 as usize]
    }
    pub fn node_mut(&mut self, id: NodeId) -> &mut Node {
        &mut self.nodes[id.0 as usize]
    }

    /// Side-table extras for a node, or `None` if the node didn't customize
    /// any of the rarely-set fields (`transform`, `position`, `grid`).
    pub fn extras(&self, id: NodeId) -> Option<&UiElementExtras> {
        self.nodes[id.0 as usize]
            .element
            .extras
            .map(|i| &self.node_extras[i as usize])
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
        &self.shapes[self.node(id).shapes_range()]
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

#[cfg(test)]
mod tests;
