use crate::element::UiElement;
use crate::primitives::{GridDef, Rect, Size, Track, TrackSlice};
use crate::shape::Shape;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct NodeId(pub(crate) u32);

#[derive(Debug)]
pub struct Node {
    /// What was recorded for this node: id, layout, mode, sense, disabled.
    /// Effective `Sense::NONE` from disabled-cascade is computed in
    /// `InputState::end_frame` by walking ancestors — `element.disabled` is
    /// just the locally-declared bit.
    pub element: UiElement,

    pub parent: Option<NodeId>,
    pub first_child: Option<NodeId>,
    pub next_sibling: Option<NodeId>,

    /// Range into Tree.shapes
    pub shapes_start: u32,
    pub shapes_end: u32,

    pub desired: Size,
    pub rect: Rect,
}

impl Node {
    /// Half-open range into `Tree.shapes` for this node's shapes. Cleaner than
    /// indexing with the raw `shapes_start`/`shapes_end` pair.
    pub fn shapes_range(&self) -> std::ops::Range<usize> {
        self.shapes_start as usize..self.shapes_end as usize
    }

    fn new(element: UiElement, parent: Option<NodeId>) -> Self {
        Self {
            element,
            parent,
            first_child: None,
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
    pub(crate) nodes: Vec<Node>,
    pub(crate) shapes: Vec<Shape>,
    /// Recording-only scratch: index `i` holds the most recently appended
    /// child of node `i`, used by `push_node` for O(1) sibling-list append.
    /// Not read after recording — kept as a parallel vec rather than a `Node`
    /// field so the Node footprint stays minimal across measure/arrange/paint.
    /// Reused frame-to-frame; cleared with `Tree::clear`.
    last_child: Vec<Option<NodeId>>,
    /// Grid track definitions, addressed by `LayoutMode::Grid(u32)`. One
    /// entry per `Grid` panel recorded this frame. Cleared with `clear`.
    grid_defs: Vec<GridDef>,
    /// Shared pool of `Track`s referenced by `GridDef::rows`/`cols` via
    /// `TrackSlice` (start, len) ranges. Cleared with `clear` so all per-grid
    /// track storage is one capacity-retaining allocation rather than two
    /// `Vec<Track>` per `Grid` per frame.
    tracks: Vec<Track>,
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
        self.tracks.clear();
    }

    /// Append a `GridDef` referencing freshly-pooled tracks; return its index.
    /// The index is stamped into a `LayoutMode::Grid(idx)` on the owning
    /// panel's `UiElement`.
    pub(crate) fn push_grid_def(
        &mut self,
        rows: &[Track],
        cols: &[Track],
        row_gap: f32,
        col_gap: f32,
    ) -> u32 {
        let row_slice = self.push_tracks(rows);
        let col_slice = self.push_tracks(cols);
        let idx = self.grid_defs.len() as u32;
        self.grid_defs.push(GridDef {
            rows: row_slice,
            cols: col_slice,
            row_gap,
            col_gap,
        });
        idx
    }

    fn push_tracks(&mut self, src: &[Track]) -> TrackSlice {
        debug_assert!(
            src.len() <= crate::primitives::MAX_TRACKS,
            "grid tracks exceed MAX_TRACKS={} (got {})",
            crate::primitives::MAX_TRACKS,
            src.len(),
        );
        let start = self.tracks.len() as u32;
        self.tracks.extend_from_slice(src);
        TrackSlice {
            start,
            len: src.len() as u32,
        }
    }

    pub(crate) fn grid_def(&self, idx: u32) -> GridDef {
        self.grid_defs[idx as usize]
    }

    pub(crate) fn grid_tracks(&self, slice: TrackSlice) -> &[Track] {
        &self.tracks[slice.range()]
    }

    pub fn push_node(&mut self, element: UiElement, parent: Option<NodeId>) -> NodeId {
        let new_id = NodeId(self.nodes.len() as u32);
        let mut node = Node::new(element, parent);
        node.shapes_start = self.shapes.len() as u32;
        node.shapes_end = self.shapes.len() as u32;
        self.nodes.push(node);
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

    /// Whether another child remains without advancing.
    pub fn has_next(&self) -> bool {
        self.next.is_some()
    }
}

#[cfg(test)]
mod tests;
