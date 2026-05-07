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
use soa_rs::{Soa, Soars};

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
/// **Per-NodeId columns** — `Soa<NodeRecord>` indexed by `NodeId.0`, in
/// pre-order paint order (parent before children, siblings in declaration
/// order). Reverse iteration gives topmost-first (used by hit-testing).
/// `soa-rs` lays each `NodeRecord` field out as its own contiguous slice,
/// so each pass touches only the bytes it needs:
///
/// - `layout`         — read by measure / arrange / alignment math
/// - `attrs`          — 1-byte packed paint/input flags; cascade / encoder
/// - `widget_id`      — hit-test, state map, damage diff
/// - `end`            — pre-order skip (every walk)
/// - `kinds`/`shapes` — encoder span lookups
///
/// **Kinds stream** — `kinds: Vec<TreeOp>` interleaves `NodeEnter`,
/// `Shape`, `NodeExit` events in pure record order. Payload lives in
/// `records` (per-NodeEnter) and `shapes` (per-Shape). The encoder and
/// hash walk this stream linearly; `records.kinds()[i]` maps a node to
/// its slice in O(1).
///
/// Recording: `open_node` pushes `NodeEnter` + a `NodeRecord`,
/// `add_shape` pushes `Shape`, `close_node` pushes `NodeExit` and
/// finalizes `records.kinds()[i]` / `records.shapes()[i]`. No
/// deferred-pending or linearization — stream and columns are final
/// the moment the root closes.
#[derive(Default)]
pub(crate) struct Tree {
    // -- Per-NodeId mandatory columns ------------------------------------
    /// SoA storage of per-NodeId data. Six parallel slices live behind
    /// one `Soa`: `widget_id`, `kinds`, `shapes`, `end`, `layout`, `attrs`.
    /// `open_node` pushes one `NodeRecord` (atomic across all six),
    /// `close_node` finalizes the `kinds`/`shapes` spans in place.
    pub(crate) records: Soa<NodeRecord>,

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
    ///   - `NodeEnter` → next entry in `records` (one push per enter)
    ///   - `Shape`     → next entry in `shapes`
    ///   - `NodeExit`  → no payload
    pub(crate) kinds: Vec<TreeOp>,
    /// Flat shape storage in record order. Indexed by counting `Shape`
    /// kinds entries up to a given position; `records.shapes()[i]`
    /// caches the range belonging to node `i`'s subtree.
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
        self.records.clear();
        self.extras.clear();
        self.chrome.clear();
        self.kinds.clear();
        self.shapes.clear();
        self.grid.clear();
        self.open_frames.clear();
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

        self.hashes.compute(
            &self.records,
            &self.extras,
            &self.chrome,
            &self.kinds,
            &self.shapes,
            &self.grid,
        );
    }

    /// Cross-column structural invariants that must hold after the
    /// root closes. One pass over `kinds` tallies the three op
    /// counts. The SoA `records` keeps every per-node column in
    /// lockstep, so a single length check is enough. Debug-only.
    #[cfg(debug_assertions)]
    fn assert_recording_invariants(&self) {
        let n = self.records.len();
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
            "kinds stream NodeEnter count diverged from records column",
        );
        debug_assert_eq!(
            exits, n,
            "kinds stream NodeExit count diverged from records column",
        );
        debug_assert_eq!(
            shape_evts,
            self.shapes.len(),
            "kinds stream Shape count diverged from shapes column",
        );
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
            extras,
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

        // Leaf marker. `close_node` rolls each closing subtree up
        // into its parent's slot, so `records.end()[i]` is final the
        // moment the root's `close_node` returns. `kinds.len` and
        // `shapes.len` are filled at close (placeholders 0); the
        // other fields are final here.
        self.records.push(NodeRecord {
            widget_id,
            kinds: Span::new(self.kinds.len() as u32, 0),
            shapes: Span::new(self.shapes.len() as u32, 0),
            end: new_id.0 + 1,
            layout,
            attrs,
        });
        self.kinds.push(TreeOp::NodeEnter);
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

        // Push NodeExit and finalize the closing node's `kinds` and
        // `shapes` spans (both placeholders set to 0 at open time).
        self.kinds.push(TreeOp::NodeExit);
        let i = closing.node.index();
        let kinds_len = self.kinds.len() as u32;
        let shapes_len = self.shapes.len() as u32;
        let kinds = &mut self.records.kinds_mut()[i];
        kinds.len = kinds_len - kinds.start;
        let shapes = &mut self.records.shapes_mut()[i];
        shapes.len = shapes_len - shapes.start;
        let end = self.records.end()[i];

        if let Some(parent) = self.open_frames.last() {
            let pi = parent.node.index();
            let ends = self.records.end_mut();
            if ends[pi] < end {
                ends[pi] = end;
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

    /// Iterate shapes attached *directly* to `node` (not its descendants),
    /// in record order. Walks `node`'s shape span, skipping each child's
    /// sub-range; relies on children's shape spans being non-overlapping
    /// and pre-order within the parent's span.
    pub(crate) fn shapes_of(&self, node: NodeId) -> impl Iterator<Item = &Shape> + '_ {
        let shapes_col = self.records.shapes();
        let mut child_subtrees = self
            .children(node)
            .map(|c| shapes_col[c.id.index()].range());
        let mut skip = child_subtrees.next();
        let shapes = &self.shapes;
        shapes_col[node.index()].range().filter_map(move |idx| {
            while let Some(r) = skip.as_ref() {
                if idx < r.start {
                    break;
                }
                if idx < r.end {
                    return None;
                }
                skip = child_subtrees.next();
            }
            Some(&shapes[idx])
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
}

pub(crate) struct ChildIter<'a> {
    tree: &'a Tree,
    next: u32,
    end: u32,
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
/// All three vecs are length `records.len()` after `end_frame`. Capacity
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

/// Per-NodeId record. One push per `open_node`, finalized by
/// `close_node`. Stored as `Soa<NodeRecord>` on `Tree.records` so each
/// field becomes its own contiguous slice — passes that read only one
/// or two fields don't pull the rest into cache.
#[derive(Soars, Clone, Copy, Debug)]
#[soa_derive(Debug)]
pub(crate) struct NodeRecord {
    /// Author-supplied identity. Read by hit-test, state map, damage diff.
    pub widget_id: WidgetId,
    /// Span into `Tree.kinds`: `kinds[start..start+len]` covers this
    /// node's `NodeEnter` through matching `NodeExit` (inclusive).
    pub kinds: Span,
    /// Span into `Tree.shapes`: covers every shape recorded inside
    /// this node's NodeEnter→NodeExit window, including descendants.
    /// `len` is set at `close_node` from `shapes.len() - shapes.start`.
    /// Stored as a `Span` (rather than just `start` + a "look at next
    /// node" trick) so a node with shapes pushed AFTER its only child
    /// closes — e.g. `Scroll` with bars at slot N — gets a correct
    /// count for the child's subtree (the next pre-order node would
    /// otherwise sit past those parent-owned bars).
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
