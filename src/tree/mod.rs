use crate::element::{Element, ElementExtras, LayoutCore, LayoutMode, PaintCore};
use crate::primitives::{Track, WidgetId};
use crate::shape::Shape;
use std::rc::Rc;

mod grid_def;
mod hash;
pub(crate) use grid_def::GridDef;
pub use hash::NodeHash;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct NodeId(pub(crate) u32);

impl NodeId {
    #[inline]
    pub fn index(self) -> usize {
        self.0 as usize
    }
}

/// Per-node columns are stored in parallel `Vec`s on `Tree`, all indexed by
/// `NodeId.0`. Splitting by reader keeps each pass touching only the bytes
/// it needs:
///
/// - `layout`     — read by measure / arrange / alignment math
/// - `paint`      — read by cascade / encoder / hit-test
/// - `widget_ids` — read only by hit-test and (future) state map
/// - `subtree_end`— pre-order topology; read by every walk
///
/// `nodes` are stored in pre-order paint order: a parent is pushed before
/// its children, and siblings appear in declaration order. Reverse iteration
/// over indices gives topmost-first traversal — load-bearing for hit-testing
/// in `Ui`.
///
/// Topology is encoded by `subtree_end[i]`: an exclusive index one past the
/// last descendant of node `i`. `i + 1 == subtree_end[i]` for a leaf.
pub struct Tree {
    pub(crate) widget_ids: Vec<WidgetId>,
    pub(crate) layout: Vec<LayoutCore>,
    pub(crate) paint: Vec<PaintCore>,
    /// Out-of-line side table for rarely-set element fields (`transform`,
    /// `position`, `grid`). `paint[i].extras` is `Some(idx)` when a node
    /// customized any of these. Cleared per frame.
    pub(crate) node_extras: Vec<ElementExtras>,
    /// Length parallel to the columns above. `i + 1 == subtree_end[i]` for a
    /// leaf or a not-yet-populated parent; otherwise points one past the last
    /// descendant of `i`.
    pub(crate) subtree_end: Vec<u32>,

    pub(crate) shapes: Vec<Shape>,
    /// Per-node shape-range starts, length always `node_count() + 1`. The
    /// shapes for node `i` are `shapes[shape_starts[i]..shape_starts[i+1]]`;
    /// the trailing sentinel is the open end of the last node, kept equal to
    /// `shapes.len()` while recording so `add_shape` can extend it in place.
    shape_starts: Vec<u32>,
    /// Recording-only scratch: index `i` holds the parent of node `i` (or
    /// `None` if root). Used by `open_node` for the ancestor-bumping walk
    /// and by `close_node` to pop `current_open`. Not read after recording.
    /// Reused frame-to-frame.
    recording_parent: Vec<Option<NodeId>>,
    /// Tip of the currently-open path while recording. `Some(n)` between an
    /// `open_node` returning `n` and the matching `close_node`. Cleared in
    /// `clear`.
    current_open: Option<NodeId>,
    /// Frame-scoped grid storage: track defs (addressed by
    /// `LayoutMode::Grid(u16)`). Per-track hug arrays live on `LayoutResult`
    /// since the tree is read-only after recording. Cleared per frame,
    /// capacity retained.
    grid: GridArena,

    /// Per-node authoring hash, computed by [`Tree::compute_hashes`] after
    /// recording is complete. Captures the inputs that affect rendering
    /// (layout fields, paint attrs, extras, shapes, grid defs) — not
    /// derived layout output (`rect`, `desired`). Damage diffs and the
    /// `TextMeasurer` reuse cache compare this against last frame's
    /// snapshot. Indexed by `NodeId.0`. Capacity retained across frames.
    pub(crate) hashes: Vec<NodeHash>,
}

impl Default for Tree {
    fn default() -> Self {
        Self {
            layout: Vec::new(),
            paint: Vec::new(),
            widget_ids: Vec::new(),
            subtree_end: Vec::new(),
            shapes: Vec::new(),
            shape_starts: vec![0],
            recording_parent: Vec::new(),
            current_open: None,
            grid: GridArena::default(),
            node_extras: Vec::new(),
            hashes: Vec::new(),
        }
    }
}

impl Tree {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn clear(&mut self) {
        self.layout.clear();
        self.paint.clear();
        self.widget_ids.clear();
        self.subtree_end.clear();
        self.shapes.clear();
        self.shape_starts.clear();
        self.shape_starts.push(0);
        self.recording_parent.clear();
        self.current_open = None;
        self.grid.clear();
        self.node_extras.clear();
        self.hashes.clear();
    }

    /// Tip of the currently-open recording path, or `None` if no node is
    /// open.
    pub fn current_open(&self) -> Option<NodeId> {
        self.current_open
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

    /// Push a node as a child of the currently-open node (or as the root if
    /// no node is open) and make it the new tip. Pair with `close_node`.
    pub fn open_node(&mut self, element: Element) -> NodeId {
        let parent = self.current_open;
        let new_id = NodeId(self.layout.len() as u32);
        if let LayoutMode::Grid(idx) = element.mode {
            assert!(
                (idx as usize) < self.grid.defs.len(),
                "LayoutMode::Grid({idx}) references no grid_def — only Grid::show should push grid nodes",
            );
        }
        let (layout, mut paint, widget_id, extras) = element.split();

        // If the parent is a `Grid`, validate the child's `GridCell` against
        // the grid's track counts now — once at recording time — instead of
        // inside every measure pass. Empty grids (zero rows or cols) skip
        // validation; their measure pass shortcuts to `Size::ZERO`.
        if let Some(parent_id) = parent
            && let LayoutMode::Grid(grid_idx) = self.layout[parent_id.0 as usize].mode
        {
            let def = self.grid.def(grid_idx);
            let n_rows = def.rows.len();
            let n_cols = def.cols.len();
            if n_rows > 0 && n_cols > 0 {
                let c = extras.grid;
                let row = c.row as usize;
                let col = c.col as usize;
                let row_span = c.row_span as usize;
                let col_span = c.col_span as usize;
                assert!(
                    row < n_rows
                        && col < n_cols
                        && row_span >= 1
                        && col_span >= 1
                        && row + row_span <= n_rows
                        && col + col_span <= n_cols,
                    "grid cell out of range: {c:?} for {n_rows}x{n_cols}"
                );
            }
        }

        if !extras.is_default() {
            assert!(
                self.node_extras.len() < u16::MAX as usize,
                "more than 65 535 nodes with extras (transform/position/grid) in a single frame",
            );
            let idx = self.node_extras.len() as u16;
            self.node_extras.push(extras);
            paint.extras = Some(idx);
        }
        self.layout.push(layout);
        self.paint.push(paint);
        self.widget_ids.push(widget_id);
        self.subtree_end.push(new_id.0 + 1);
        self.shape_starts.push(self.shapes.len() as u32);
        self.recording_parent.push(parent);

        // Walk up the ancestor chain, growing each one's `subtree_end` so the
        // new node falls inside every ancestor's subtree. Cheap in practice:
        // typical UI trees are shallow.
        let new_end = new_id.0 + 1;
        let mut anc = parent;
        while let Some(a) = anc {
            let ai = a.0 as usize;
            self.subtree_end[ai] = new_end;
            anc = self.recording_parent[ai];
        }
        self.current_open = Some(new_id);
        new_id
    }

    /// Pop the currently-open node back to its parent. Panics if no node is
    /// open.
    pub fn close_node(&mut self) {
        let cur = self
            .current_open
            .expect("close_node called with no open node");
        self.current_open = self.recording_parent[cur.0 as usize];
    }

    pub fn add_shape(&mut self, node: NodeId, shape: Shape) {
        let idx = node.0 as usize;
        assert_eq!(
            idx,
            self.node_count() - 1,
            "shapes for node {idx} must be added contiguously, before any child node",
        );
        self.shapes.push(shape);
        *self.shape_starts.last_mut().unwrap() = self.shapes.len() as u32;
    }

    pub fn layout(&self, id: NodeId) -> &LayoutCore {
        &self.layout[id.0 as usize]
    }

    pub fn paint(&self, id: NodeId) -> PaintCore {
        self.paint[id.0 as usize]
    }

    pub fn is_collapsed(&self, id: NodeId) -> bool {
        self.layout[id.0 as usize].is_collapsed()
    }

    pub fn node_count(&self) -> usize {
        self.layout.len()
    }

    /// Direct access to the layout column. Use when you need to iterate every
    /// node's layout fields in storage order.
    pub fn layouts(&self) -> &[LayoutCore] {
        &self.layout
    }

    /// Direct access to the paint column.
    pub fn paints(&self) -> &[PaintCore] {
        &self.paint
    }

    /// Direct access to the subtree-end column.
    pub fn subtree_ends(&self) -> &[u32] {
        &self.subtree_end
    }

    /// Read extras for a node, returning a borrow of `ElementExtras::DEFAULT`
    /// when the node has no side-table entry. Use this when you want to read
    /// individual fields (`gap`, `child_align`, `position`, …) without
    /// duplicating defaults at every call site.
    pub fn read_extras(&self, id: NodeId) -> &ElementExtras {
        match self.paint[id.0 as usize].extras {
            Some(i) => &self.node_extras[i as usize],
            None => &ElementExtras::DEFAULT,
        }
    }

    /// First node in pre-order paint order, or `None` if the tree is empty.
    /// Stable while the tree is alive: the root is always `NodeId(0)` once
    /// pushed.
    pub fn root(&self) -> Option<NodeId> {
        if self.layout.is_empty() {
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
            subtree_end: &self.subtree_end,
            next: parent.0 + 1,
            end: self.subtree_end[pi],
        }
    }

    /// Iterate non-collapsed child NodeIds of `parent` in declaration order.
    /// Layout drivers measure/intrinsic loops use this to skip the
    /// `if tree.is_collapsed(c) { continue; }` boilerplate. Arrange loops
    /// generally still need the explicit branch because collapsed children
    /// affect cursor/gap bookkeeping differently.
    pub fn children_active(&self, parent: NodeId) -> impl Iterator<Item = NodeId> + '_ {
        self.children(parent).filter(|&c| !self.is_collapsed(c))
    }

    /// Authoring hash for `id`. `NodeHash::UNCOMPUTED` if
    /// [`Self::compute_hashes`] hasn't run yet this frame. Damage and
    /// text-reuse compare this against last frame's hash for the same
    /// `WidgetId` to detect per-node authoring changes.
    pub fn node_hash(&self, id: NodeId) -> NodeHash {
        self.hashes
            .get(id.index())
            .copied()
            .unwrap_or(NodeHash::UNCOMPUTED)
    }

    /// Walk every recorded node and populate `self.hashes`. Pure read
    /// over the rest of the tree; safe to call any time after recording
    /// completes. Capacity retained across frames.
    pub(crate) fn compute_hashes(&mut self) {
        let n = self.node_count();
        self.hashes.clear();
        self.hashes.reserve(n);
        for i in 0..n {
            let layout = &self.layout[i];
            let paint = self.paint[i];
            let extras = paint.extras.map(|idx| &self.node_extras[idx as usize]);
            let s_start = self.shape_starts[i] as usize;
            let s_end = self.shape_starts[i + 1] as usize;
            let shapes = &self.shapes[s_start..s_end];
            let grid_def = match layout.mode {
                LayoutMode::Grid(idx) => Some(&self.grid.defs[idx as usize]),
                _ => None,
            };
            self.hashes.push(hash::compute_node_hash(
                layout, paint, extras, shapes, grid_def,
            ));
        }
    }
}

pub struct ChildIter<'a> {
    subtree_end: &'a [u32],
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
        self.next = self.subtree_end[self.next as usize];
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
