use crate::layout::types::track::Track;
use crate::primitives::background::Background;
use crate::shape::Shape;
use crate::tree::element::{
    Element, ElementExtras, ElementSplit, LayoutCore, LayoutMode, PaintCore,
};
use crate::tree::node_hash::NodeHash;
use crate::tree::widget_id::WidgetId;
use rustc_hash::FxHasher;
use std::hash::Hasher;
use std::rc::Rc;

pub(crate) mod element;
pub(crate) mod node_hash;
pub(crate) mod widget_id;

/// Track definitions + axis gaps for a `Grid` panel. Stored on `GridArena`
/// (a `Tree`-owned `Vec<GridDef>`) and addressed from
/// `LayoutMode::Grid(u16)`. Track defs live behind `Rc<[Track]>` so callers
/// can cache and share them across frames without the framework copying —
/// the builder stores the `Rc`, the layout pass reads through it directly.
/// Per-track hug sizes (computed in measure, read in arrange) live on
/// `LayoutResult` keyed by grid def index — the tree is read-only after
/// recording.
#[derive(Clone, Debug)]
pub(crate) struct GridDef {
    pub rows: Rc<[Track]>,
    pub cols: Rc<[Track]>,
    pub row_gap: f32,
    pub col_gap: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct NodeId(pub(crate) u32);

impl NodeId {
    #[inline]
    pub(crate) fn index(self) -> usize {
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
#[derive(Default)]
pub(crate) struct Tree {
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

    /// Per-node `Shape` storage (flat buffer + per-node start offsets).
    /// Reads via `shapes.slice_of(idx)`; writes via [`Self::add_shape`].
    pub(crate) shapes: ShapeBuf,
    /// Recording-only scratch: index `i` holds the parent of node `i`,
    /// or `Self::NO_PARENT` (`u32::MAX`) for a root. Used by
    /// `close_node` to pop `current_open` and by `finalize_subtree_end`
    /// for the post-record rollup. `u32` with a sentinel halves the
    /// footprint vs `Vec<Option<NodeId>>` (8 bytes / entry) and lets
    /// the rollup loop branch on a plain integer compare.
    recording_parent: Vec<u32>,
    /// Tip of the currently-open path while recording. `Some(n)` between an
    /// `open_node` returning `n` and the matching `close_node`. Cleared in
    /// `clear`.
    pub(crate) current_open: Option<NodeId>,
    /// Frame-scoped grid storage: track defs (addressed by
    /// `LayoutMode::Grid(u16)`). Per-track hug arrays live on `LayoutResult`
    /// since the tree is read-only after recording. Cleared per frame,
    /// capacity retained.
    pub(crate) grid: GridArena,

    /// Per-node authoring hash + subtree-rollup hash + grid-presence bit,
    /// all populated by [`Self::end_frame`]. Indexed by `NodeId.0`,
    /// capacity retained across frames.
    pub(crate) hashes: NodeHashes,
}

impl Tree {
    /// Sentinel parent for root nodes in `recording_parent`. Picked at
    /// `u32::MAX` so a valid `NodeId.0` (capped at `node_count() - 1`)
    /// never collides.
    const NO_PARENT: u32 = u32::MAX;

    pub(crate) fn begin_frame(&mut self) {
        self.layout.clear();
        self.paint.clear();
        self.widget_ids.clear();
        self.subtree_end.clear();
        self.shapes.begin_frame();
        self.recording_parent.clear();
        self.current_open = None;
        self.grid.clear();
        self.node_extras.clear();
        self.hashes.begin_frame();
    }

    /// Walk every recorded node and populate `self.hashes.node` plus the
    /// `self.hashes.subtree` rollup. Pure read over the rest of the
    /// tree; safe to call any time after recording completes. Capacity
    /// retained across frames.
    pub(crate) fn end_frame(&mut self) {
        self.finalize_subtree_end();

        let n = self.layout.len();

        let nodes = &mut self.hashes.node;
        nodes.clear();
        nodes.reserve(n);
        for i in 0..n {
            let layout = &self.layout[i];
            let paint = self.paint[i];
            let extras = paint.extras.map(|idx| &self.node_extras[idx as usize]);
            let shapes = self.shapes.slice_of(i);
            let grid_def = match layout.mode {
                LayoutMode::Grid(idx) => Some(&self.grid.defs[idx as usize]),
                _ => None,
            };
            nodes.push(NodeHash::compute(layout, paint, extras, shapes, grid_def));
        }

        // Subtree-hash rollup. Pre-order arena means every child has a
        // strictly higher index than its parent, so iterating in
        // reverse fills children before their parent reads them. Each
        // parent folds its own node-hash with its direct children's
        // subtree hashes, in declaration order — sibling reorder
        // changes the parent's subtree hash.
        self.hashes.subtree.clear();
        self.hashes.subtree.resize(n, NodeHash::UNCOMPUTED);
        self.hashes.subtree_has_grid.clear();
        self.hashes.subtree_has_grid.resize(n, false);
        for i in (0..n).rev() {
            let end = self.subtree_end[i];
            let mut h = FxHasher::default();
            h.write_u64(self.hashes.node[i].as_u64());
            // Fold `transform` into `subtree_hash` (but not `node[i]`):
            // damage diffs descendants' screen rects, so the per-node
            // hash stays transform-insensitive. The encode cache, which
            // keys on `subtree` and replays cached `PushTransform`
            // bytes verbatim, must invalidate on transform changes.
            if let Some(t) = self.read_extras(NodeId(i as u32)).transform {
                h.write_u8(1);
                let bytes = bytemuck::bytes_of(&t);
                h.write(bytes);
            } else {
                h.write_u8(0);
            }
            let mut has_grid = matches!(self.layout[i].mode, LayoutMode::Grid(_));
            let mut next = (i as u32) + 1;
            while next < end {
                h.write_u64(self.hashes.subtree[next as usize].as_u64());
                has_grid |= self.hashes.subtree_has_grid[next as usize];
                next = self.subtree_end[next as usize];
            }
            self.hashes.subtree[i] = NodeHash::from_u64(h.finish());
            self.hashes.subtree_has_grid[i] = has_grid;
        }
    }

    /// Roll `subtree_end` up from leaves to roots so every internal
    /// node's slot points one past its last descendant. After recording,
    /// `subtree_end[i]` is the per-node leaf marker `i + 1` (set in
    /// `open_node`); this single reverse pass uses `recording_parent` to
    /// propagate each child's `subtree_end` up to its parent. Pre-order
    /// arena → children always have higher indices than their parent →
    /// reverse iteration visits children first. Idempotent.
    pub(crate) fn finalize_subtree_end(&mut self) {
        let n = self.layout.len();
        let parents = &self.recording_parent[..n];
        let ends = &mut self.subtree_end[..n];
        for i in (1..n).rev() {
            let p = parents[i];
            if p == Self::NO_PARENT {
                continue;
            }
            let pi = p as usize;
            if ends[pi] < ends[i] {
                ends[pi] = ends[i];
            }
        }
    }

    /// Push a node as a child of the currently-open node (or as the root if
    /// no node is open) and make it the new tip. Pair with `close_node`.
    pub(crate) fn open_node(
        &mut self,
        element: Element,
        chrome: Option<Background>,
    ) -> NodeId {
        let parent = self.current_open;
        let new_id = NodeId(self.layout.len() as u32);
        if let LayoutMode::Grid(idx) = element.mode {
            assert!(
                (idx as usize) < self.grid.defs.len(),
                "LayoutMode::Grid({idx}) references no grid_def — only Grid::show should push grid nodes",
            );
        }
        let ElementSplit {
            layout,
            mut paint,
            id: widget_id,
            mut extras,
        } = element.split();
        // Chrome is the per-node-call surface paint. Element doesn't
        // carry it; `ui.node` threads it here so we can stamp it onto
        // `extras.chrome` before the side-table allocation check —
        // an extras-bearing chrome on an otherwise-default element
        // still gets a side-table slot allocated.
        extras.chrome = chrome;

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
        // Provisional leaf marker; `finalize_subtree_end` rolls it up
        // post-recording. Walking ancestors per `open_node` was an
        // O(N·depth) random-write loop with a true data dependency on
        // `recording_parent`; the deferred pass is O(N) sequential.
        self.subtree_end.push(new_id.0 + 1);
        self.shapes.open_node();
        self.recording_parent
            .push(parent.map_or(Self::NO_PARENT, |p| p.0));
        self.current_open = Some(new_id);
        new_id
    }

    /// Pop the currently-open node back to its parent. Panics if no node is
    /// open.
    pub(crate) fn close_node(&mut self) {
        let cur = self
            .current_open
            .expect("close_node called with no open node");
        let p = self.recording_parent[cur.0 as usize];
        self.current_open = if p == Self::NO_PARENT {
            None
        } else {
            Some(NodeId(p))
        };
    }

    pub(crate) fn push_grid_def(&mut self, def: GridDef) -> u16 {
        self.grid.push_def(def)
    }

    pub(crate) fn add_shape(&mut self, node: NodeId, shape: Shape) {
        let idx = node.0 as usize;
        assert_eq!(
            idx,
            self.layout.len() - 1,
            "shapes for node {idx} must be added contiguously, before any child node",
        );
        // Multi-`Shape::Text` per leaf is unsupported: layout records a
        // single `ShapedText` per node and the encoder emits a single
        // `DrawText` rect — a second text shape would silently
        // overwrite the first's shaped buffer / cache key. Catch at
        // authoring time rather than letting the corruption land in
        // `LayoutResult.text_shapes`.
        if matches!(shape, Shape::Text { .. }) {
            assert!(
                !self
                    .shapes
                    .slice_of(idx)
                    .iter()
                    .any(|s| matches!(s, Shape::Text { .. })),
                "node {idx} already has a Shape::Text — multiple text shapes per leaf are unsupported",
            );
        }
        self.shapes.push_to_open(shape);
    }

    pub(crate) fn is_collapsed(&self, id: NodeId) -> bool {
        self.layout[id.0 as usize].visibility.is_collapsed()
    }

    /// Read extras for a node, returning a borrow of `ElementExtras::DEFAULT`
    /// when the node has no side-table entry. Use this when you want to read
    /// individual fields (`gap`, `child_align`, `position`, …) without
    /// duplicating defaults at every call site.
    pub(crate) fn read_extras(&self, id: NodeId) -> &ElementExtras {
        match self.paint[id.0 as usize].extras {
            Some(i) => &self.node_extras[i as usize],
            None => &ElementExtras::DEFAULT,
        }
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

    /// Iterate child NodeIds of `parent` in declaration order.
    pub(crate) fn children(&self, parent: NodeId) -> ChildIter<'_> {
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
    /// affect cursor/gap bookkeeping differently — see [`Self::children_with_state`].
    pub(crate) fn children_active(&self, parent: NodeId) -> impl Iterator<Item = NodeId> + '_ {
        self.children(parent).filter(|&c| !self.is_collapsed(c))
    }

    /// Iterate child NodeIds of `parent` tagged with their collapse state.
    /// Used by every arrange driver: collapsed children must still be
    /// visited (so their subtree's rects get zeroed at the parent's
    /// anchor) but contribute nothing to cursors or content size.
    /// Replacing the per-driver `if tree.is_collapsed(c) {…} continue;`
    /// boilerplate.
    pub(crate) fn children_with_state(&self, parent: NodeId) -> impl Iterator<Item = Child> + '_ {
        self.children(parent).map(|c| {
            if self.is_collapsed(c) {
                Child::Collapsed(c)
            } else {
                Child::Active(c)
            }
        })
    }

    /// Subtree authoring hash for `id`: rolls this node's hash with
    /// its descendants' (in declaration order). `NodeHash::UNCOMPUTED`
    /// before [`Self::end_frame`] runs.
    pub(crate) fn subtree_hash(&self, id: NodeId) -> NodeHash {
        self.hashes
            .subtree
            .get(id.index())
            .copied()
            .unwrap_or(NodeHash::UNCOMPUTED)
    }
}

pub(crate) struct ChildIter<'a> {
    subtree_end: &'a [u32],
    next: u32,
    end: u32,
}

/// Child of a parent, tagged with its collapse state. Yielded by
/// [`Tree::children_with_state`]; the dispatch on this enum replaces
/// the `if tree.is_collapsed(c) {…} continue;` boilerplate that used
/// to live in every arrange driver.
#[derive(Copy, Clone, Debug)]
pub(crate) enum Child {
    /// Visible / measured child — drive its layout normally.
    Active(NodeId),
    /// Collapsed child — its subtree must be zeroed at the parent's
    /// anchor and skipped from cursor/content bookkeeping.
    Collapsed(NodeId),
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
    pub(crate) defs: Vec<GridDef>,
}

impl GridArena {
    fn clear(&mut self) {
        self.defs.clear();
    }

    /// Append a `GridDef` referencing user-owned `Rc<[Track]>` rows + cols;
    /// return its index. The index is stamped into a `LayoutMode::Grid(idx)`
    /// on the owning panel's `Element`. `Rc` clones are refcount-only.
    fn push_def(&mut self, def: GridDef) -> u16 {
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
    pub(crate) subtree_has_grid: Vec<bool>,
}

impl NodeHashes {
    fn begin_frame(&mut self) {
        self.node.clear();
        self.subtree.clear();
        self.subtree_has_grid.clear();
    }
}

/// Per-node `Shape` storage: a flat `buf` of all shapes plus a `starts`
/// table where `buf[starts[i]..starts[i+1]]` is node `i`'s slice.
/// `starts` always has length `node_count() + 1` — the trailing
/// sentinel is the open end of the most recently opened node, kept
/// equal to `buf.len()` while recording so [`Self::push_to_open`] can
/// extend it in place.
pub(crate) struct ShapeBuf {
    buf: Vec<Shape>,
    starts: Vec<u32>,
}

impl Default for ShapeBuf {
    fn default() -> Self {
        Self {
            buf: Vec::new(),
            starts: vec![0],
        }
    }
}

impl ShapeBuf {
    fn begin_frame(&mut self) {
        self.buf.clear();
        self.starts.clear();
        self.starts.push(0);
    }

    /// Slice of shapes for node index `node_idx`. Empty for nodes that
    /// recorded no shapes.
    pub(crate) fn slice_of(&self, node_idx: usize) -> &[Shape] {
        let s = self.starts[node_idx] as usize;
        let e = self.starts[node_idx + 1] as usize;
        &self.buf[s..e]
    }

    /// Mark a fresh open node — its shape range starts at the current
    /// buffer end and extends as [`Self::push_to_open`] appends.
    fn open_node(&mut self) {
        self.starts.push(self.buf.len() as u32);
    }

    /// Append `shape` to the most-recently-opened node's range. Caller
    /// (`Tree::add_shape`) enforces the contiguity + Text-uniqueness
    /// invariants before delegating here.
    fn push_to_open(&mut self, shape: Shape) {
        self.buf.push(shape);
        *self.starts.last_mut().unwrap() = self.buf.len() as u32;
    }
}

#[cfg(test)]
mod tests;
