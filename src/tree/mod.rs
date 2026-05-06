use crate::common::hash::Hasher;
use crate::common::sparse_column::SparseColumn;
use crate::layout::types::span::Span;
use crate::layout::types::visibility::Visibility;
use crate::primitives::background::Background;
use crate::shape::Shape;
use crate::tree::element::{
    Element, ElementExtras, ElementSplit, LayoutCore, LayoutMode, PaintAttrs,
};
use crate::tree::node_hash::NodeHash;
use crate::tree::widget_id::WidgetId;
use crate::widgets::grid::GridDef;
use fixedbitset::FixedBitSet;

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

/// Two storage modes coexist:
///
/// **Per-NodeId columns** — parallel `Vec`s indexed by `NodeId.0`, all in
/// pre-order paint order (parent before children, siblings in declaration
/// order). Reverse iteration gives topmost-first (used by hit-testing).
/// Splitting by reader keeps each pass touching only the bytes it needs:
///
/// - `layout`     — read by measure / arrange / alignment math
/// - `attrs`      — packed paint/input flags (sense / disabled /
///   clip / focusable). 1 byte/node; read by cascade / encoder /
///   hit-test in dense columnar passes
/// - `nodes`      — per-NodeId `NodeMeta`, bundling the author's
///   `widget_id`, the kinds-stream `Span`, the shape-stream `Span`
///   covering this node's subtree, and the exclusive end NodeId.
///   `i + 1 == nodes[i].end` for a leaf.
///
/// **Kinds stream** — `kinds: Vec<TreeOp>` interleaves `NodeEnter`,
/// `Shape`, `NodeExit` events in pure record order. Payload lives in
/// `nodes` + `layout` + `attrs` (per-NodeEnter) and `shapes`
/// (per-Shape). The encoder and hash walk this stream linearly;
/// `nodes[i].kinds` maps a node to its slice in O(1).
///
/// Recording: `open_node` pushes `NodeEnter` + per-node columns,
/// `add_shape` pushes `Shape`, `close_node` pushes `NodeExit` and
/// finalizes `nodes[i]`. No deferred-pending or linearization —
/// stream and columns are final the moment the root closes.
#[derive(Default)]
pub(crate) struct Tree {
    // -- Per-NodeId mandatory columns ------------------------------------
    /// Per-NodeId metadata bundle:
    /// - `widget_id` — author-supplied identity (hit-test, state map).
    /// - `kinds`     — `Span` covering this node's `NodeEnter` through
    ///   matching `NodeExit` in the kinds stream.
    /// - `shapes`    — `Span` covering shapes recorded inside this
    ///   node's NodeEnter→NodeExit window (this node + descendants).
    /// - `end`       — exclusive end NodeId in pre-order
    ///   (`i + 1 == nodes[i].end` for a leaf).
    ///
    /// Bundled because all four are written together at `open_node` /
    /// `close_node` and the bookkeeping ends would risk desync if
    /// split.
    // todo soa
    pub(crate) nodes: Vec<NodeMeta>,
    /// Layout-pass column: geometry + visibility. Held tight so the
    /// hot measure/arrange path doesn't pull paint flags it never
    /// reads.
    pub(crate) layout: Vec<LayoutCore>,
    /// 1-byte packed paint/input flags per node (sense / disabled /
    /// clip / focusable). Split from `layout` so cascade / encoder /
    /// hit-test pull a 16-nodes-per-cacheline dense column instead of
    /// the full `LayoutCore`.
    pub(crate) attrs: Vec<PaintAttrs>,

    // -- Per-NodeId sparse side tables -----------------------------------
    /// Rarely-set element fields (`transform`, `position`, `grid`, …)
    /// stored sparsely — most nodes leave them all default. The
    /// `SparseColumn` packs a per-NodeId index column with the dense
    /// table; cleared per frame, capacity retained.
    pub(crate) extras: SparseColumn<ElementExtras>,
    /// Chrome (panel `Background`) stored sparsely. Decoupled from
    /// `extras` because chrome is panel-common while the rest of
    /// `ElementExtras` is rare — bundling them was costing every
    /// chrome-bearing panel a 108-byte extras slot to hold a single
    /// `Option<Background>`.
    pub(crate) chrome: SparseColumn<Background>,

    // -- Kinds stream + flat shape buffer --------------------------------
    /// Tagged event stream interleaving node enters/exits with shapes,
    /// in pure record order. Walked linearly by the encoder; replaces
    /// the old per-node shape slice + slot mechanism. Each entry maps
    /// to a payload:
    ///   - `NodeEnter` → next entry in `nodes[i].widget_id` /
    ///     `layout[i]` / `attrs[i]`
    ///   - `Shape`     → next entry in `shapes`
    ///   - `NodeExit`  → no payload
    pub(crate) kinds: Vec<TreeOp>,
    /// Flat shape storage in record order. Indexed by counting `Shape`
    /// kinds entries up to a given position; `nodes[i].shapes` caches
    /// the range belonging to node `i`'s subtree.
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
    /// Per-node authoring hash + subtree-rollup hash + grid-presence bit,
    /// all populated by [`Self::end_frame`]. Indexed by `NodeId.0`,
    /// capacity retained across frames.
    pub(crate) hashes: NodeHashes,
}

impl Tree {
    pub(crate) fn begin_frame(&mut self) {
        self.nodes.clear();
        self.layout.clear();
        self.attrs.clear();
        self.extras.clear();
        self.chrome.clear();
        self.kinds.clear();
        self.shapes.clear();
        self.grid.clear();
        self.open_frames.clear();
        self.hashes.begin_frame();
    }

    /// Walk every recorded node and populate `self.hashes.node` plus the
    /// `self.hashes.subtree` rollup. Pure read over the rest of the
    /// tree; safe to call any time after recording completes. Capacity
    /// retained across frames.
    pub(crate) fn end_frame(&mut self) {
        assert!(
            self.open_frames.is_empty(),
            "end_frame called with {} node(s) still open — a widget builder forgot close_node",
            self.open_frames.len(),
        );
        #[cfg(debug_assertions)]
        self.assert_recording_invariants();

        // Per-node + subtree hashes, both populated by a single
        // entry point on `NodeHashes`. `mem::take` swaps the storage
        // out so `compute` can read from `self` and write into the
        // local without borrow conflicts; capacity is preserved.
        // todo untangle
        let mut hashes = std::mem::take(&mut self.hashes);
        hashes.compute(self);
        self.hashes = hashes;
    }

    /// Cross-column structural invariants that must hold after the
    /// root closes. One pass over `kinds` tallies the three op
    /// counts; `nodes` and `attrs` length matches are direct asserts.
    /// Catches drift between the recording stream and the per-NodeId
    /// columns the moment a `Tree` mutation gets it wrong. Debug-only.
    #[cfg(debug_assertions)]
    fn assert_recording_invariants(&self) {
        let n = self.layout.len();
        let mut enters = 0usize;
        let mut exits = 0usize;
        let mut shape_evts = 0usize;
        for op in &self.kinds {
            match op {
                TreeOp::NodeEnter => enters += 1,
                TreeOp::NodeExit => exits += 1,
                TreeOp::Shape => shape_evts += 1,
            }
        }
        debug_assert_eq!(
            enters, n,
            "kinds stream NodeEnter count diverged from layout column",
        );
        debug_assert_eq!(
            exits, n,
            "kinds stream NodeExit count diverged from layout column",
        );
        debug_assert_eq!(
            shape_evts,
            self.shapes.len(),
            "kinds stream Shape count diverged from shapes column",
        );
        debug_assert_eq!(
            self.nodes.len(),
            n,
            "nodes column length must match layout column after recording",
        );
        debug_assert_eq!(
            self.attrs.len(),
            n,
            "attrs column length must match layout column after recording",
        );
    }

    /// Push a node as a child of the currently-open node (or as the root if
    /// no node is open) and make it the new tip. Pair with `close_node`.
    pub(crate) fn open_node(&mut self, element: Element, chrome: Option<Background>) -> NodeId {
        let parent = self.open_frames.last().map(|f| f.node);
        let new_id = NodeId(self.layout.len() as u32);
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
            extras,
        } = element.split();

        // If the parent is a `Grid`, validate the child's `GridCell` against
        // the grid's track counts now — once at recording time — instead of
        // inside every measure pass. Empty grids (zero rows or cols) skip
        // validation; their measure pass shortcuts to `Size::ZERO`.
        if let Some(parent_id) = parent
            && let LayoutMode::Grid(grid_idx) = self.layout[parent_id.0 as usize].mode
        {
            let def = &self.grid.defs[grid_idx as usize];
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

        self.extras.push((!extras.is_default()).then_some(extras));
        self.chrome.push(chrome);

        self.layout.push(layout);
        self.attrs.push(attrs);
        // Leaf marker. `close_node` rolls each closing subtree up
        // into its parent's slot, so `nodes[i].end` is final the
        // moment the root's `close_node` returns. `kinds.len` and
        // `shapes.len` are filled at close (placeholders 0); the
        // other fields are final here.
        self.nodes.push(NodeMeta {
            widget_id,
            kinds: Span::new(self.kinds.len() as u32, 0),
            shapes: Span::new(self.shapes.len() as u32, 0),
            end: new_id.0 + 1,
        });
        self.kinds.push(TreeOp::NodeEnter);
        self.open_frames.push(OpenFrame {
            node: new_id,
            has_text: false,
        });
        new_id
    }

    /// Pop the currently-open node back to its parent and roll its
    /// `nodes[…].end` up into the parent's slot. Panics if no node
    /// is open.
    pub(crate) fn close_node(&mut self) {
        let closing = self
            .open_frames
            .pop()
            .expect("close_node called with no open node");

        // Push NodeExit and finalize the closing node's `kinds` and
        // `shapes` spans (both placeholders set to 0 at open time).
        self.kinds.push(TreeOp::NodeExit);
        let meta = &mut self.nodes[closing.node.index()];
        meta.kinds.len = self.kinds.len() as u32 - meta.kinds.start;
        meta.shapes.len = self.shapes.len() as u32 - meta.shapes.start;
        let end = meta.end;

        if let Some(parent) = self.open_frames.last() {
            let pi = parent.node.index();
            if self.nodes[pi].end < end {
                self.nodes[pi].end = end;
            }
        }
    }

    /// Append a shape to the currently-open tip. The shape's position
    /// in the kinds stream encodes its paint order — shapes pushed
    /// before any child paint first, shapes between two children paint
    /// between them, etc. Targeting is positional (whichever `Ui` is
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
        self.kinds.push(TreeOp::Shape);
        self.shapes.push(shape);
    }

    /// First node in pre-order paint order, or `None` if the tree is empty.
    /// Stable while the tree is alive: the root is always `NodeId(0)` once
    /// pushed.
    pub(crate) fn root(&self) -> Option<NodeId> {
        if self.layout.is_empty() {
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
            layout: &self.layout,
            nodes: &self.nodes,
            next: parent.0 + 1,
            end: self.nodes[pi].end,
        }
    }

    // -- Per-node read accessors -----------------------------------------

    /// Iterate shapes attached *directly* to `node` (not its descendants),
    /// in record order. Walks the inside of node `i`'s `kinds` span,
    /// yielding depth-0 `Shape` events.
    pub(crate) fn shapes_of(&self, node: NodeId) -> impl Iterator<Item = &Shape> + '_ {
        let i = node.index();
        let meta = &self.nodes[i];
        let r = meta.kinds.range();
        let start = r.start + 1;
        let end = r.end - 1;
        let shapes = &self.shapes;
        let mut depth = 0i32;
        let mut shape_cursor = meta.shapes.start as usize;
        self.kinds[start..end]
            .iter()
            .filter_map(move |op| match op {
                TreeOp::NodeEnter => {
                    depth += 1;
                    None
                }
                TreeOp::NodeExit => {
                    depth -= 1;
                    None
                }
                TreeOp::Shape => {
                    let idx = shape_cursor;
                    shape_cursor += 1;
                    (depth == 0).then_some(&shapes[idx])
                }
            })
    }

    /// Read extras for a node, returning a borrow of `ElementExtras::DEFAULT`
    /// when the node has no side-table entry. Use this when you want to read
    /// individual fields (`gap`, `child_align`, `position`, …) without
    /// duplicating defaults at every call site.
    pub(crate) fn read_extras(&self, id: NodeId) -> &ElementExtras {
        self.extras
            .get(id.index())
            .unwrap_or(&ElementExtras::DEFAULT)
    }

    /// Chrome for `id`, or `None` if the node has no chrome.
    pub(crate) fn chrome_for(&self, id: NodeId) -> Option<&Background> {
        self.chrome.get(id.index())
    }

    /// Subtree authoring hash for `id`: rolls this node's hash with
    /// its descendants' (in declaration order). Panics if called before
    /// [`Self::end_frame`] has populated the column for this frame —
    /// callers should never observe `NodeHash::UNCOMPUTED` outside the
    /// rollup loop's own scratch.
    pub(crate) fn subtree_hash(&self, id: NodeId) -> NodeHash {
        self.hashes.subtree[id.index()]
    }
}

// todo have only tree ref
pub(crate) struct ChildIter<'a> {
    layout: &'a [LayoutCore],
    nodes: &'a [NodeMeta],
    next: u32,
    end: u32,
}

/// Child of a parent, tagged with its `Visibility`. Yielded by
/// [`Tree::children`]; replaces the
/// `if tree.is_collapsed(c) {…} continue;` boilerplate that used to
/// live in every arrange driver. `visibility` is the cached value of
/// `tree.layout[id.index()].visibility` — read once during iteration.
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
        let visibility = self.layout[id.index()].visibility;
        self.next = self.nodes[self.next as usize].end;
        Some(Child { id, visibility })
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

/// Per-node hash data populated by [`Tree::end_frame`].
///
/// - `node[i]` — authoring hash of node `i` alone (layout / paint /
///   extras / shapes / grid def). Read by damage diff and the leaf
///   intrinsic cache.
/// - `subtree[i]` — rollup of `node[i]` together with the subtree
///   hashes of `i`'s direct children, in declaration order. Equality
///   across frames means nothing in the subtree changed; the cross-frame
///   measure cache and encode cache both key on this. See
///   `src/layout/measure-cache.md` and
///   `src/renderer/frontend/encoder/encode-cache.md`.
/// - `subtree_has_grid[i]` — true if the subtree at `i` contains any
///   `LayoutMode::Grid` node. Fast-path skip for `MeasureCache`'s
///   grid-hug snapshot/restore walk; correctness doesn't depend on it,
///   perf does.
///
/// All three vecs are length `node_count()` after `end_frame`. Capacity
/// retained across frames.
#[derive(Default)]
pub(crate) struct NodeHashes {
    pub(crate) node: Vec<NodeHash>,
    pub(crate) subtree: Vec<NodeHash>,
    pub(crate) subtree_has_grid: FixedBitSet,
    /// Reused scratch for the single-pass per-node hash compute (one
    /// entry per currently-open ancestor). Capacity peaks at tree
    /// depth and is retained across frames so the hot path is alloc-
    /// free in steady state.
    pub(crate) compute_stack: Vec<(NodeId, Hasher)>,
}

impl NodeHashes {
    fn begin_frame(&mut self) {
        self.node.clear();
        self.subtree.clear();
        self.subtree_has_grid.clear();
    }
}

/// Per-NodeId metadata bundle. Holds everything that's both
/// (a) one-per-node and (b) finalized by `close_node`. Layout-side
/// data lives separately in `Tree.layout` because the layout pass
/// reads it densely and shouldn't pull these unrelated bytes.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct NodeMeta {
    /// Author-supplied identity. Read by hit-test and the state map.
    pub(crate) widget_id: WidgetId,
    /// Span into `Tree.kinds`: `kinds[start..start+len]` covers this
    /// node's `NodeEnter` through matching `NodeExit` (inclusive).
    pub(crate) kinds: Span,
    /// Span into `Tree.shapes`: `shapes[start..start+len]` covers
    /// every shape recorded inside this node's NodeEnter→NodeExit
    /// window, including descendants. `len` is set at `close_node`
    /// from `shapes.len() - shapes.start`. Stored as a `Span`
    /// (rather than just `start` + a "look at next node" trick) so a
    /// node with shapes pushed AFTER its only child closes — e.g.
    /// `Scroll` with bars at slot N — gets a correct count for the
    /// child's subtree (the next pre-order node would otherwise sit
    /// past those parent-owned bars).
    pub(crate) shapes: Span,
    /// Exclusive end in NodeId space: one past the last descendant
    /// in pre-order. `i + 1 == end` for a leaf.
    pub(crate) end: u32,
}

/// Tagged event in the per-frame `kinds` stream — see `Tree.kinds`.
/// One byte each. `NodeEnter` and `NodeExit` always pair (root-first
/// preorder); `Shape` events sit between, in record order.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TreeOp {
    NodeEnter,
    Shape,
    NodeExit,
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
