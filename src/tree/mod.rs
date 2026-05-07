use crate::common::hash::Hasher;
use crate::common::sparse_column::SparseColumn;
use crate::layout::types::span::Span;
use crate::layout::types::visibility::Visibility;
use crate::primitives::background::Background;
use crate::shape::Shape;
use crate::tree::element::{
    BoundsExtras, Element, ElementSplit, LayoutCore, LayoutMode, PaintAttrs, PanelExtras,
};
use crate::tree::node_hash::{NodeHash, NodeHashes};
use crate::tree::widget_id::WidgetId;
use crate::widgets::grid::GridDef;
use fixedbitset::FixedBitSet;
use soa_rs::{Soa, Soars};
use std::hash::{Hash, Hasher as _};

pub(crate) mod element;
pub(crate) mod node_hash;
pub(crate) mod widget_id;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct NodeId(pub(crate) u32);

impl NodeId {
    #[inline]
    pub(crate) fn index(self) -> usize {
        self.0 as usize
    }
}

/// **Per-NodeId columns** — `Soa<NodeRecord>` indexed by `NodeId.0`, in
/// pre-order paint order (parent before children, siblings in declaration
/// order). Reverse iteration gives topmost-first (used by hit-testing).
/// `soa-rs` lays each `NodeRecord` field out as its own contiguous slice,
/// so each pass touches only the bytes it needs:
///
/// - `layout`    — read by measure / arrange / alignment math
/// - `attrs`     — 1-byte packed paint/input flags; cascade / encoder
/// - `widget_id` — hit-test, state map, damage diff
/// - `end`       — pre-order skip (every walk)
/// - `shapes`    — span into the flat shape buffer covering this node's
///   subtree (parent + descendants); the gap between children's
///   sub-ranges holds the parent's direct shapes in record order.
///
/// Record order between direct shapes and child enters is recoverable
/// from `shapes.start` — each child captures the shape buffer length at
/// its open, so a parent shape pushed before child C appears at index
/// `< C.shapes.start` and one pushed after C closes appears at index
/// `>= C.shapes.start + C.shapes.len`. Encoder and hash both walk
/// `tree.children(id)` in declaration order and emit shapes from the
/// gaps; no separate event stream needed.
///
/// Recording: `open_node` pushes a `NodeRecord`, `add_shape` pushes
/// onto the flat shape buffer, `close_node` finalizes `shapes` len.
/// No deferred-pending or linearization — columns are final the moment
/// the root closes.
#[derive(Default)]
pub(crate) struct Tree {
    // -- Per-NodeId mandatory columns ------------------------------------
    /// SoA storage of per-NodeId data. Five parallel slices live behind
    /// one `Soa`: `widget_id`, `shapes`, `end`, `layout`, `attrs`.
    /// `open_node` pushes one `NodeRecord` (atomic across all five),
    /// `close_node` finalizes the `shapes` span in place.
    pub(crate) records: Soa<NodeRecord>,

    // -- Per-NodeId sparse side tables -----------------------------------
    /// Per-node bounds + transform + parent-relative placement (`min_size`,
    /// `max_size`, `position`, `grid`, `transform`). Sparse — most leaves
    /// leave them all default. Split from panel-only fields so leaves that
    /// only set bounds don't bloat their slot with `gap`/`justify`/etc.
    pub(crate) bounds: SparseColumn<BoundsExtras>,
    /// Panel-only knobs (`gap`, `line_gap`, `justify`, `child_align`).
    /// Sparse — leaves never write these, so the column stays small even in
    /// large trees; ~16B/entry vs the old ~64B unified extras.
    pub(crate) panel: SparseColumn<PanelExtras>,
    /// Chrome (panel `Background`) stored sparsely. Decoupled from the
    /// extras columns because chrome is panel-common while bounds/panel
    /// fields are mostly default.
    pub(crate) chrome: SparseColumn<Background>,

    // -- Flat shape buffer -----------------------------------------------
    /// Flat shape storage in record order. `records.shapes()[i]` is the
    /// range belonging to node `i`'s subtree (parent + all descendants);
    /// the gaps between children's sub-ranges hold the parent's direct
    /// shapes in record order.
    pub(crate) shapes: Vec<Shape>,

    // -- Frame-scoped sub-storage ----------------------------------------
    /// Frame-scoped grid storage: track defs (addressed by
    /// `LayoutMode::Grid(u16)`). Per-track hug arrays live on `LayoutResult`
    /// since the tree is read-only after recording. Cleared per frame,
    /// capacity retained.
    pub(crate) grid: GridArena,

    // -- Recording-only state --------------------------------------------
    /// Stack of currently-open nodes. The last entry is the tip;
    /// preceding entries are its ancestors. Capacity peaks at tree depth
    /// — typically a handful of entries. Empty outside the
    /// `begin_frame` ↔ root `close_node` window.
    open_frames: Vec<OpenFrame>,

    // -- Output (populated by `end_frame`) -------------------------------
    /// Per-node + subtree-rollup authoring hashes, populated by
    /// [`Self::end_frame`]. Indexed by `NodeId.0`, capacity retained
    /// across frames.
    pub(crate) hashes: NodeHashes,
    /// Bit `i` is true iff the subtree rooted at node `i` contains any
    /// `LayoutMode::Grid` node. Fast-path skip for `MeasureCache`'s
    /// grid-hug snapshot/restore walk; populated by [`Self::end_frame`]
    /// alongside the rollup. Conceptually a structure summary, not a
    /// hash, hence kept off `NodeHashes`.
    pub(crate) subtree_has_grid: FixedBitSet,
}

impl Tree {
    pub(crate) fn begin_frame(&mut self) {
        self.records.clear();
        self.bounds.clear();
        self.panel.clear();
        self.chrome.clear();
        self.shapes.clear();
        self.grid.clear();
        self.open_frames.clear();
        self.subtree_has_grid.clear();
    }

    /// Finalize the recorded frame: walk every node and populate
    /// `self.hashes.node` + `self.hashes.subtree`. Pure read over the
    /// rest of the tree; safe to call any time after recording
    /// completes. Capacity retained across frames.
    pub(crate) fn end_frame(&mut self) {
        assert!(
            self.open_frames.is_empty(),
            "end_frame called with {} node(s) still open — a widget builder forgot close_node",
            self.open_frames.len(),
        );
        #[cfg(debug_assertions)]
        self.assert_recording_invariants();

        self.compute_node_hashes();
        self.compute_subtree_hashes();
    }

    /// Per-node authoring hash. Hashes layout / extras / chrome, then
    /// walks `direct_items` interleaving each direct shape with a
    /// `0xFF` marker per child to preserve "shape vs child boundary"
    /// ordering across siblings.
    fn compute_node_hashes(&mut self) {
        let n = self.records.len();
        self.hashes.node.clear();
        self.hashes.node.reserve(n);

        for i in 0..n {
            let mut h = Hasher::new();
            self.records.layout()[i].hash(&mut h);
            self.records.attrs()[i].hash(&mut h);
            if let Some(b) = self.bounds.get(i) {
                b.hash(&mut h);
            }
            if let Some(p) = self.panel.get(i) {
                p.hash(&mut h);
            }
            self.chrome.get(i).hash(&mut h);

            for item in TreeItems::new(&self.records, &self.shapes, NodeId(i as u32)) {
                match item {
                    TreeItem::Shape(s) => s.hash(&mut h),
                    TreeItem::Child(_) => h.write_u8(0xFF),
                }
            }

            if let LayoutMode::Grid(idx) = self.records.layout()[i].mode {
                self.grid.defs[idx as usize].hash(&mut h);
            }
            self.hashes.node.push(NodeHash(h.finish()));
        }
    }

    /// Subtree-hash rollup, reverse pre-order so children fill before
    /// parents read. `transform` folds in here (not into `node[i]`) so
    /// the encode cache invalidates on transform-only changes while
    /// damage rect-diffing keeps owning paint-position drift.
    fn compute_subtree_hashes(&mut self) {
        let n = self.records.len();
        self.hashes.subtree.clear();
        self.hashes.subtree.resize_with(n, NodeHash::default);

        for i in (0..n).rev() {
            let end = self.records.end()[i];
            let mut h = Hasher::new();
            h.write_u64(self.hashes.node[i].0);
            if let Some(t) = self.bounds.get(i).and_then(|b| b.transform) {
                h.write_u8(1);
                h.pod(&t);
            } else {
                h.write_u8(0);
            }
            let mut next = (i as u32) + 1;
            while next < end {
                h.write_u64(self.hashes.subtree[next as usize].0);
                next = self.records.end()[next as usize];
            }
            self.hashes.subtree[i] = NodeHash(h.finish());
        }
    }

    /// Cross-column structural invariants. The root's `shapes` span
    /// must cover the entire shape buffer — every shape lives in some
    /// subtree. Debug-only.
    #[cfg(debug_assertions)]
    fn assert_recording_invariants(&self) {
        if let Some(root) = self.root() {
            let root_shapes = self.records.shapes()[root.index()];
            debug_assert_eq!(root_shapes.start, 0, "root shapes span must start at 0",);
            debug_assert_eq!(
                root_shapes.len as usize,
                self.shapes.len(),
                "root shapes span must cover the whole shape buffer",
            );
        }
    }

    /// Push a node as a child of the currently-open node (or as the root if
    /// no node is open) and make it the new tip. Pair with `close_node`.
    pub(crate) fn open_node(&mut self, element: Element, chrome: Option<Background>) -> NodeId {
        let parent = self.open_frames.last().map(|f| f.node);
        let new_id = NodeId(self.records.len() as u32);
        if let LayoutMode::Grid(idx) = element.mode {
            assert!(
                (idx as usize) < self.grid.defs.len(),
                "LayoutMode::Grid({idx}) references no grid_def — only Grid::show should push grid nodes",
            );
        }
        let ElementSplit {
            layout,
            attrs,
            id: widget_id,
            bounds,
            panel,
        } = element.split();

        // If the parent is a `Grid`, validate the child's `GridCell` against
        // the grid's track counts now — once at recording time — instead of
        // inside every measure pass. Empty grids (zero rows or cols) skip
        // validation; their measure pass shortcuts to `Size::ZERO`.
        if let Some(parent_id) = parent
            && let LayoutMode::Grid(grid_idx) = self.records.layout()[parent_id.0 as usize].mode
        {
            let def = &self.grid.defs[grid_idx as usize];
            let n_rows = def.rows.len();
            let n_cols = def.cols.len();
            if n_rows > 0 && n_cols > 0 {
                let c = bounds.grid;
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

        self.bounds.push((!bounds.is_default()).then_some(bounds));
        self.panel.push((!panel.is_default()).then_some(panel));
        self.chrome.push(chrome);

        // Leaf marker. `close_node` rolls each closing subtree up
        // into its parent's slot, so `records.end()[i]` is final the
        // moment the root's `close_node` returns. `shapes.len` is
        // filled at close (placeholder 0); the other fields are final
        // here.
        self.records.push(NodeRecord {
            widget_id,
            shapes: Span::new(self.shapes.len() as u32, 0),
            end: new_id.0 + 1,
            layout,
            attrs,
        });
        self.subtree_has_grid.grow(self.records.len());
        self.open_frames.push(OpenFrame {
            node: new_id,
            has_text: false,
        });
        new_id
    }

    /// Pop the currently-open node back to its parent and roll its
    /// `records.end()[…]` up into the parent's slot. Panics if no node
    /// is open.
    pub(crate) fn close_node(&mut self) {
        let closing = self
            .open_frames
            .pop()
            .expect("close_node called with no open node");

        // Finalize the closing node's `shapes` span (placeholder set
        // to 0 at open time).
        let i = closing.node.index();
        let shapes_len = self.shapes.len() as u32;
        let shapes = &mut self.records.shapes_mut()[i];
        shapes.len = shapes_len - shapes.start;
        let end = self.records.end()[i];

        // Roll up `subtree_has_grid`: this node's bit is true iff its
        // own layout is `Grid` or any descendant's bit is set (set as
        // grandchildren closed). After deciding for `i`, OR into the
        // parent's bit so the chain propagates to the root.
        if matches!(self.records.layout()[i].mode, LayoutMode::Grid(_)) {
            self.subtree_has_grid.insert(i);
        }
        let i_has_grid = self.subtree_has_grid.contains(i);

        if let Some(parent) = self.open_frames.last() {
            let pi = parent.node.index();
            let ends = self.records.end_mut();
            if ends[pi] < end {
                ends[pi] = end;
            }
            if i_has_grid {
                self.subtree_has_grid.insert(pi);
            }
        }
    }

    /// Append a shape to the currently-open tip. The shape's position
    /// in the flat buffer encodes its paint order — shapes pushed
    /// before any child end up at indices below the child's
    /// `shapes.start`, shapes between two children sit between their
    /// sub-ranges, etc. Targeting is positional (whichever `Ui` is
    /// open).
    pub(crate) fn add_shape(&mut self, shape: Shape) {
        let tip = self
            .open_frames
            .last_mut()
            .expect("add_shape called with no open node");
        // Multi-`Shape::Text` per leaf is unsupported: layout records a
        // single `ShapedText` per node and the encoder emits a single
        // `DrawText` rect — a second text shape would silently
        // overwrite the first's shaped buffer / cache key. Catch at
        // authoring time rather than letting the corruption land in
        // `LayoutResult.text_shapes`.
        if matches!(shape, Shape::Text { .. }) {
            assert!(
                !tip.has_text,
                "node {} already has a Shape::Text — multiple text shapes per leaf are unsupported",
                tip.node.0,
            );
            tip.has_text = true;
        }
        self.shapes.push(shape);
    }

    /// First node in pre-order paint order, or `None` if the tree is empty.
    /// Stable while the tree is alive: the root is always `NodeId(0)` once
    /// pushed.
    pub(crate) fn root(&self) -> Option<NodeId> {
        if self.records.is_empty() {
            None
        } else {
            Some(NodeId(0))
        }
    }

    /// Iterate children of `parent` in declaration order, each tagged
    /// with its collapse state (`Child::Active` / `Child::Collapsed`).
    /// Use `.filter_map(Child::active)` for active-only iteration, or
    /// match on `Child` when collapsed children still need handling
    /// (arrange loops zero their subtrees at the parent's anchor).
    pub(crate) fn children(&self, parent: NodeId) -> ChildIter<'_> {
        let pi = parent.0 as usize;
        ChildIter {
            tree: self,
            next: parent.0 + 1,
            end: self.records.end()[pi],
        }
    }

    // -- Per-node read accessors -----------------------------------------

    /// Iterate `node`'s direct contents in record order. Wrapper over
    /// [`TreeItems::new`] for callers that hold a `&Tree`; the
    /// hash compute reaches for `TreeItems::new` directly so it can
    /// keep `&mut self.hashes` live.
    pub(crate) fn tree_items(&self, node: NodeId) -> TreeItems<'_> {
        TreeItems::new(&self.records, &self.shapes, node)
    }

    /// Direct shapes only — convenience over `direct_items` for
    /// callers that don't care about child boundaries (mostly tests
    /// and `leaf_text_shapes`, which only runs on leaves).
    pub(crate) fn shapes_of(&self, node: NodeId) -> impl Iterator<Item = &Shape> + '_ {
        self.tree_items(node).filter_map(|item| match item {
            TreeItem::Shape(s) => Some(s),
            TreeItem::Child(_) => None,
        })
    }

    /// Bounds extras for a node (`min_size`, `max_size`, `position`, `grid`,
    /// `transform`), falling back to `BoundsExtras::DEFAULT` when the node has
    /// no side-table entry.
    pub(crate) fn bounds(&self, id: NodeId) -> &BoundsExtras {
        self.bounds
            .get(id.index())
            .unwrap_or(&BoundsExtras::DEFAULT)
    }

    /// Panel extras for a node (`gap`, `line_gap`, `justify`, `child_align`),
    /// falling back to `PanelExtras::DEFAULT` when the node has no entry.
    /// Leaves never set these, so the column stays small.
    pub(crate) fn panel(&self, id: NodeId) -> &PanelExtras {
        self.panel.get(id.index()).unwrap_or(&PanelExtras::DEFAULT)
    }

    /// Chrome for `id`, or `None` if the node has no chrome.
    pub(crate) fn chrome_for(&self, id: NodeId) -> Option<&Background> {
        self.chrome.get(id.index())
    }
}

pub(crate) struct ChildIter<'a> {
    tree: &'a Tree,
    next: u32,
    end: u32,
}

/// One step in the record-order walk of a node's direct contents.
/// `Child` carries the visibility tag the same way `ChildIter` does;
/// callers that don't need it (e.g. encoder) just use `child.id`.
#[derive(Copy, Clone, Debug)]
pub(crate) enum TreeItem<'a> {
    Shape(&'a Shape),
    Child(Child),
}

/// Child of a parent, tagged with its `Visibility`. Yielded by
/// [`Tree::children`]; replaces the
/// `if tree.is_collapsed(c) {…} continue;` boilerplate that used to
/// live in every arrange driver. `visibility` is the cached value of
/// `tree.records.layout()[id.index()].visibility` — read once during
/// iteration.
#[derive(Copy, Clone, Debug)]
pub(crate) struct Child {
    pub(crate) id: NodeId,
    pub(crate) visibility: Visibility,
}

impl Child {
    /// `Some(id)` when the child is active (not collapsed), `None`
    /// when collapsed. Pairs with `.filter_map(Child::active)` for
    /// active-only loops. Hidden counts as active here — it still
    /// participates in layout (occupies space), only paint/hit-test
    /// short-circuit at the cascade.
    #[inline]
    pub(crate) fn active(self) -> Option<NodeId> {
        (!self.visibility.is_collapsed()).then_some(self.id)
    }
}

impl<'a> Iterator for ChildIter<'a> {
    type Item = Child;
    fn next(&mut self) -> Option<Child> {
        if self.next >= self.end {
            return None;
        }
        let id = NodeId(self.next);
        let visibility = self.tree.records.layout()[id.index()].visibility;
        self.next = self.tree.records.end()[self.next as usize];
        Some(Child { id, visibility })
    }
}

/// Iterator over a node's direct contents in record order. Yields
/// each direct shape interleaved with each direct child at the
/// position the child was opened. Construct via [`TreeItems::new`]
/// from the column slices, or [`Tree::direct_items`] from a `&Tree`.
/// The two-constructor surface exists so methods that need
/// `&mut self.hashes` can still iterate without a `&self` borrow.
pub(crate) struct TreeItems<'a> {
    shapes_col: &'a [Span],
    layouts: &'a [LayoutCore],
    ends: &'a [u32],
    shapes: &'a [Shape],
    cursor: usize,
    parent_end: usize,
    next_child_id: u32,
    subtree_end: u32,
}

impl<'a> TreeItems<'a> {
    pub(crate) fn new(records: &'a Soa<NodeRecord>, shapes: &'a [Shape], node: NodeId) -> Self {
        let shapes_col = records.shapes();
        let parent = shapes_col[node.index()];
        Self {
            shapes_col,
            layouts: records.layout(),
            ends: records.end(),
            shapes,
            cursor: parent.start as usize,
            parent_end: (parent.start + parent.len) as usize,
            next_child_id: node.0 + 1,
            subtree_end: records.end()[node.index()],
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
                let s = &self.shapes[self.cursor];
                self.cursor += 1;
                return Some(TreeItem::Shape(s));
            }
            let visibility = self.layouts[self.next_child_id as usize].visibility;
            let child = Child {
                id: NodeId(self.next_child_id),
                visibility,
            };
            self.cursor = cs_start + cs.len as usize;
            self.next_child_id = self.ends[self.next_child_id as usize];
            return Some(TreeItem::Child(child));
        }
        if self.cursor < self.parent_end {
            let s = &self.shapes[self.cursor];
            self.cursor += 1;
            return Some(TreeItem::Shape(s));
        }
        None
    }
}

/// Frame-scoped grid storage: track defs (one per `Grid` panel),
/// addressed by `LayoutMode::Grid(u16)`. Per-track hug arrays live on
/// `LayoutResult` since the tree is read-only after recording.
/// Capacity is retained across frames; data is cleared per frame.
#[derive(Default)]
pub(crate) struct GridArena {
    pub(crate) defs: Vec<GridDef>,
}

impl GridArena {
    fn clear(&mut self) {
        self.defs.clear();
    }

    /// Append a `GridDef` referencing user-owned `Rc<[Track]>` rows +
    /// cols; return its index. The index is stamped into a
    /// `LayoutMode::Grid(idx)` on the owning panel's `Element`. `Rc`
    /// clones are refcount-only.
    pub(crate) fn push_def(&mut self, def: GridDef) -> u16 {
        assert!(
            self.defs.len() < u16::MAX as usize,
            "more than 65 535 Grid panels in a single frame",
        );
        let idx = self.defs.len() as u16;
        self.defs.push(def);
        idx
    }
}

/// Per-NodeId record. One push per `open_node`, finalized by
/// `close_node`. Stored as `Soa<NodeRecord>` on `Tree.records` so each
/// field becomes its own contiguous slice — passes that read only one
/// or two fields don't pull the rest into cache.
#[derive(Soars, Clone, Copy, Debug)]
#[soa_derive(Debug)]
pub(crate) struct NodeRecord {
    /// Author-supplied identity. Read by hit-test, state map, damage diff.
    pub widget_id: WidgetId,
    /// Span into `Tree.shapes`: covers every shape recorded inside
    /// this node's open→close window, including descendants. `len` is
    /// set at `close_node` from `shapes.len() - shapes.start`. Stored
    /// as a `Span` (rather than just `start` + a "look at next node"
    /// trick) so a node with shapes pushed AFTER its only child closes
    /// — e.g. `Scroll` with bars at slot N — gets a correct count for
    /// the child's subtree.
    pub shapes: Span,
    /// Exclusive end in NodeId space: one past the last descendant
    /// in pre-order. `i + 1 == end` for a leaf.
    pub end: u32,
    /// Layout-pass column: geometry + visibility. Bundled because the
    /// hot measure/arrange path reads all six fields together.
    pub layout: LayoutCore,
    /// 1-byte packed paint/input flags (sense / disabled / clip /
    /// focusable). Read by cascade / encoder / hit-test.
    pub attrs: PaintAttrs,
}

/// Recording-only frame for the open-stack. One per currently-open
/// node, root-first; the last entry is the tip.
#[derive(Clone, Copy, Debug)]
struct OpenFrame {
    /// The node this frame represents.
    node: NodeId,
    /// Tracks "this node already has a `Shape::Text`" — multi-Text per
    /// leaf is unsupported (the layout pass writes a single
    /// `ShapedText` slot per node). Avoids an O(node-shapes) scan in
    /// `add_shape`.
    has_text: bool,
}

#[cfg(test)]
mod tests;
