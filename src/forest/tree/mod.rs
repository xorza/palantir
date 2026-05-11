use crate::ClipMode;
use crate::common::hash::Hasher;
use crate::common::sparse_column::SparseColumn;
use crate::forest::element::{
    BoundsExtras, Element, ElementSplit, LayoutCore, LayoutMode, PanelExtras,
};
use crate::forest::node::NodeRecord;
use crate::forest::rollups::{NodeHash, SubtreeRollups};
use crate::forest::shapes::ShapeRecord;
use crate::forest::shapes::Shapes;
use crate::forest::visibility::Visibility;
use crate::layout::types::span::Span;
use crate::primitives::background::Background;
use crate::primitives::corners::Corners;
use crate::primitives::rect::Rect;
use crate::primitives::size::Size;
use crate::widgets::grid::GridDef;
use glam::Vec2;
use soa_rs::Soa;
use std::hash::{Hash, Hasher as _};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct NodeId(pub(crate) u32);

impl NodeId {
    /// Sentinel "no parent" value used in [`Tree::parents`] for root
    /// slots. `u32::MAX` is unreachable as a real `NodeId` (record cap
    /// is `u32::MAX - 1` in practice; sparse column caps trip far
    /// sooner).
    pub(crate) const ROOT: Self = Self(u32::MAX);

    #[inline]
    pub(crate) fn index(self) -> usize {
        self.0 as usize
    }
}

/// Paint / hit-test order across layers. Lower variants paint first
/// (under) and hit-test last (under). Total order — popups beat the
/// main tree, modals beat popups, tooltips beat modals, debug beats
/// everything. See `docs/popups.md`.
///
/// `#[repr(u8)]` + the contiguous variant layout means `layer as usize`
/// is a valid index into `[T; Layer::COUNT]` per-layer storage. With
/// the forest topology each variant owns its own [`Tree`] arena.
#[repr(u8)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, strum::EnumCount)]
pub enum Layer {
    #[default]
    Main = 0,
    Popup = 1,
    Modal = 2,
    Tooltip = 3,
    Debug = 4,
}

impl Layer {
    /// Paint order (low → high). Iterate trees in this order so layers
    /// paint bottom-up; reverse for topmost-first hit-test traversal.
    pub(crate) const PAINT_ORDER: [Layer; <Layer as strum::EnumCount>::COUNT] = [
        Layer::Main,
        Layer::Popup,
        Layer::Modal,
        Layer::Tooltip,
        Layer::Debug,
    ];
}

/// One entry on `Tree::open_frames`. Carries the open node's
/// `NodeId` plus a `disabled` cascade bit propagated at push time
/// (`parent.ancestor_or_self_disabled || new_node.disabled`) so
/// `Tree::ancestor_disabled` is an O(1) read.
#[derive(Clone, Copy, Debug)]
pub(crate) struct OpenFrame {
    pub(crate) node: NodeId,
    pub(crate) ancestor_or_self_disabled: bool,
}

/// One root within a single layer's [`Tree`]. Multiple roots in the
/// same tree happen for popups (eater + body recorded as two
/// top-level scopes) and any future `Ui::layer` scope that opens
/// non-contiguous top-level subtrees in the same layer.
#[derive(Clone, Copy, Debug)]
pub(crate) struct RootSlot {
    pub(crate) first_node: u32,
    /// `min` = caller-supplied top-left placement (surface origin for
    /// `Main`). `size` = the room from that placement to the surface
    /// bottom-right, so layout's "available" is bounded by the
    /// surface even for side-layer roots. Patched in
    /// `Forest::end_frame`; consumed by `LayoutEngine::run` per root.
    pub(crate) anchor_rect: Rect,
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
/// Each [`Tree`] is a single layer's arena. Per-layer trees live on
/// [`forest::Forest`] and share no record/shape storage — mid-recording
/// `Ui::layer` calls dispatch into the destination tree without
/// interleaving, eliminating the prior reorder pass.
#[derive(Default)]
pub(crate) struct Tree {
    // -- Per-NodeId mandatory columns ------------------------------------
    pub(crate) records: Soa<NodeRecord>,

    // -- Per-NodeId sparse side tables -----------------------------------
    pub(crate) bounds: SparseColumn<BoundsExtras>,
    pub(crate) panel: SparseColumn<PanelExtras>,
    pub(crate) chrome: SparseColumn<Background>,
    /// Mask radius for nodes whose `clip` is `ClipMode::Rounded`.
    /// Decoupled from `chrome` so that a node with rounded clip but
    /// invisible paint (or no paint at all) still has a radius for the
    /// encoder's stencil-mask path. Set in `open_node` from the
    /// element's clip mode; absent for `ClipMode::None` / `Rect`.
    pub(crate) clip_radius: SparseColumn<Corners>,

    /// Parent `NodeId` per node, or [`NodeId::ROOT`] for roots. Written
    /// at `open_node` from `open_frames.last()`; lets any post-recording
    /// pass (arrange, cascade, encode, debug) ask "who's my parent?" in
    /// O(1) without a backwards `subtree_end` walk. Same lifecycle as
    /// `records`: cleared in `begin_frame`, pushed in `open_node`,
    /// length-asserted at the end of `open_node`.
    pub(crate) parents: Vec<NodeId>,

    // -- Shapes ----------------------------------------------------------
    /// Flat per-frame shape buffer (`shapes.records`) + per-variant
    /// side-table payloads (`shapes.payloads`). Records are indexed
    /// via `NodeRecord.shape_span`; payloads back the variable-length
    /// `Mesh` / `Polyline` variants.
    pub(crate) shapes: Shapes,

    // -- Frame-scoped sub-storage ----------------------------------------
    pub(crate) grid: GridArena,

    // -- Roots -----------------------------------------------------------
    /// Top-level root slots in this tree, in record order. Each slot's
    /// `first_node` indexes `records`; pipeline passes iterate the
    /// slice. Empty when no nodes were recorded into this tree this
    /// frame.
    pub(crate) roots: Vec<RootSlot>,

    // -- Recording-only ancestor stack -----------------------------------
    /// Ancestor stack for this tree's currently-open scope. Empty
    /// outside the `begin_frame` ↔ root `close_node` window. Capacity
    /// retained.
    ///
    /// Each frame carries a precomputed `ancestor_or_self_disabled`
    /// bit: on push, OR the new node's `disabled` with the parent
    /// frame's bit. That makes `ancestor_disabled` a one-element
    /// load (read from `last()`) instead of an O(depth) walk.
    pub(crate) open_frames: Vec<OpenFrame>,

    /// Top-left placement that the next `open_node` will stamp on a
    /// freshly-minted `RootSlot` (as `anchor_rect.min`). Set by
    /// `Forest::push_layer` for non-`Main` layers; `Main`'s value
    /// stays at `Vec2::ZERO` until `Forest::end_frame` patches every
    /// `Main` root's anchor to the surface. The root's
    /// `anchor_rect.size` is patched in `end_frame` to the remaining
    /// surface extent (`surface - anchor`), so layout sees the
    /// available room from the placement onwards.
    pub(crate) pending_anchor: Vec2,

    // -- Output (populated by `end_frame`) -------------------------------
    pub(crate) rollups: SubtreeRollups,
}

impl Tree {
    pub(crate) fn begin_frame(&mut self) {
        self.records.clear();
        self.bounds.clear();
        self.panel.clear();
        self.chrome.clear();
        self.clip_radius.clear();
        self.parents.clear();
        self.shapes.clear();
        self.grid.clear();
        self.rollups.has_grid.clear();
        self.roots.clear();
        self.open_frames.clear();
        self.pending_anchor = Vec2::ZERO;
    }

    /// Finalize this tree: populate `rollups.node` + `rollups.subtree`.
    /// Capacity retained across frames.
    pub(crate) fn end_frame(&mut self) {
        assert!(
            self.open_frames.is_empty(),
            "end_frame called with {} node(s) still open — a widget builder forgot close_node",
            self.open_frames.len(),
        );
        self.rollups.reset_hashes_for(self.records.len());
        self.compute_node_hashes();
        self.compute_subtree_hashes();
    }

    fn compute_node_hashes(&mut self) {
        let n = self.records.len();
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
            let chrome = self.chrome.get(i);
            chrome.hash(&mut h);
            self.clip_radius.get(i).hash(&mut h);
            let mut has_direct_shape = false;
            for item in TreeItems::new(&self.records, &self.shapes.records, NodeId(i as u32)) {
                match item {
                    TreeItem::ShapeRecord(s) => {
                        has_direct_shape = true;
                        s.hash(&mut h);
                    }
                    TreeItem::Child(_) => h.write_u8(0xFF),
                }
            }
            if chrome.is_some() || has_direct_shape {
                self.rollups.paints.set(i, true);
            }

            if let LayoutMode::Grid(idx) = self.records.layout()[i].mode {
                self.grid.defs[idx as usize].hash(&mut h);
            }
            self.rollups.node.push(NodeHash(h.finish()));
        }
    }

    fn compute_subtree_hashes(&mut self) {
        let n = self.records.len();
        for i in (0..n).rev() {
            let end = self.records.subtree_end()[i];
            let mut h = Hasher::new();
            h.write_u64(self.rollups.node[i].0);
            if let Some(t) = self.bounds.get(i).and_then(|b| b.transform) {
                h.write_u8(1);
                h.pod(&t);
            } else {
                h.write_u8(0);
            }
            let mut next = (i as u32) + 1;
            while next < end {
                h.write_u64(self.rollups.subtree[next as usize].0);
                next = self.records.subtree_end()[next as usize];
            }
            self.rollups.subtree[i] = NodeHash(h.finish());
        }
    }

    /// Push a node as a child of the currently-open node (or as a new
    /// root if `open_frames` is empty) and make it the new tip. Root
    /// mints stamp `self.pending_anchor` onto the new `RootSlot`;
    /// child opens don't read the anchor.
    pub(crate) fn open_node(&mut self, mut element: Element) -> NodeId {
        let parent_frame = self.open_frames.last().copied();
        let parent = parent_frame.map(|f| f.node);
        let new_id = NodeId(self.records.len() as u32);
        if parent.is_none() {
            self.roots.push(RootSlot {
                first_node: new_id.0,
                // `size` is patched in `Forest::end_frame` to the
                // remaining surface extent from `pending_anchor`.
                anchor_rect: Rect {
                    min: self.pending_anchor,
                    size: Size::ZERO,
                },
            });
        }
        if let LayoutMode::Grid(idx) = element.mode {
            assert!(
                (idx as usize) < self.grid.defs.len(),
                "LayoutMode::Grid({idx}) references no grid_def — only Grid::show should push grid nodes",
            );
        }
        // `ClipMode::Rounded` without a usable radius — caller asked
        // for a stencil mask but didn't supply corners (no chrome, or
        // chrome with all-zero radius). Downgrade to scissor: the
        // stencil pass would mask exactly the rect bbox anyway, and
        // skipping it saves a render pass. Mirrors what users wrote
        // by hand before the API split.
        let chrome = element.chrome;
        if matches!(element.clip, ClipMode::Rounded)
            && chrome.is_none_or(|bg| bg.radius.approx_zero())
        {
            element.clip = ClipMode::Rect;
        }
        let ElementSplit {
            layout,
            attrs,
            id: widget_id,
            bounds,
            panel,
        } = element.split();

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
        // Single noop-policy site for chrome:
        //   * `clip_radius` extracts `chrome.radius` whenever this
        //     node has `ClipMode::Rounded`, *regardless* of whether
        //     the paint is invisible — the encoder needs the radius
        //     for the stencil mask even when the paint isn't drawn.
        //   * `chrome` itself is dropped to `None` when the paint is
        //     invisible (`Background::is_noop()`), so the encoder can
        //     just say "if chrome is some, draw it" — no per-frame
        //     noop guard downstream.
        // Together these decouple "paint info" from "mask radius",
        // so the encoder treats both as plain plumbing.
        let clip_radius = matches!(attrs.clip_mode(), ClipMode::Rounded)
            .then(|| chrome.as_ref().map(|bg| bg.radius))
            .flatten();
        let chrome_for_paint = chrome.filter(|bg| !bg.is_noop());
        self.chrome.push(chrome_for_paint);
        self.clip_radius.push(clip_radius);

        self.records.push(NodeRecord {
            widget_id,
            shape_span: Span::new(self.shapes.records.len() as u32, 0),
            subtree_end: new_id.0 + 1,
            layout,
            attrs,
        });
        self.parents.push(parent.unwrap_or(NodeId::ROOT));
        self.rollups.has_grid.grow(self.records.len());
        // Column length-equality. `records` + four sparse + `parents`
        // must agree on `len`; a missed push silently shifts every
        // later node's index. soa-rs guards the records' six fields;
        // this guards the rest.
        let n = self.records.len();
        assert_eq!(self.bounds.idx.len(), n);
        assert_eq!(self.panel.idx.len(), n);
        assert_eq!(self.chrome.idx.len(), n);
        assert_eq!(self.clip_radius.idx.len(), n);
        assert_eq!(self.parents.len(), n);
        let ancestor_or_self_disabled =
            parent_frame.is_some_and(|f| f.ancestor_or_self_disabled) || attrs.is_disabled();
        self.open_frames.push(OpenFrame {
            node: new_id,
            ancestor_or_self_disabled,
        });
        new_id
    }

    /// True when any currently-open ancestor in this tree's recording
    /// scope has `disabled=true`. Lets widgets see inherited-disabled
    /// at record time, in the *same* frame the ancestor was opened —
    /// `cascade.disabled` is one frame stale, so without this an
    /// inherited-disabled child paints alive on first appearance and
    /// then animates to disabled. O(1): the bit is propagated on
    /// `open_node` push, so `last()` already encodes the OR over the
    /// whole open chain.
    pub(crate) fn ancestor_disabled(&self) -> bool {
        self.open_frames
            .last()
            .is_some_and(|f| f.ancestor_or_self_disabled)
    }

    pub(crate) fn close_node(&mut self) {
        let closing = self
            .open_frames
            .pop()
            .expect("close_node called with no open node")
            .node;

        let i = closing.index();
        let shapes_len = self.shapes.records.len() as u32;
        let shapes = &mut self.records.shape_span_mut()[i];
        shapes.len = shapes_len - shapes.start;
        let end = self.records.subtree_end()[i];

        if matches!(self.records.layout()[i].mode, LayoutMode::Grid(_)) {
            self.rollups.has_grid.insert(i);
        }
        let i_has_grid = self.rollups.has_grid.contains(i);

        if let Some(parent) = self.open_frames.last().map(|f| f.node) {
            let pi = parent.index();
            let ends = self.records.subtree_end_mut();
            if ends[pi] < end {
                ends[pi] = end;
            }
            if i_has_grid {
                self.rollups.has_grid.insert(pi);
            }
        }
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    /// Iterate children of `parent` in declaration order, each tagged
    /// with its collapse state (`Child::Active` / `Child::Collapsed`).
    /// Use `.filter_map(Child::active)` for active-only iteration.
    pub(crate) fn children(&self, parent: NodeId) -> ChildIter<'_> {
        let pi = parent.0 as usize;
        ChildIter {
            tree: self,
            next: parent.0 + 1,
            end: self.records.subtree_end()[pi],
        }
    }

    pub(crate) fn tree_items(&self, node: NodeId) -> TreeItems<'_> {
        TreeItems::new(&self.records, &self.shapes.records, node)
    }

    pub(crate) fn bounds(&self, id: NodeId) -> &BoundsExtras {
        self.bounds
            .get(id.index())
            .unwrap_or(&BoundsExtras::DEFAULT)
    }

    pub(crate) fn panel(&self, id: NodeId) -> &PanelExtras {
        self.panel.get(id.index()).unwrap_or(&PanelExtras::DEFAULT)
    }
}

pub(crate) struct ChildIter<'a> {
    tree: &'a Tree,
    next: u32,
    end: u32,
}

#[derive(Copy, Clone, Debug)]
pub(crate) enum TreeItem<'a> {
    ShapeRecord(&'a ShapeRecord),
    Child(Child),
}

#[derive(Copy, Clone, Debug)]
pub(crate) struct Child {
    pub(crate) id: NodeId,
    pub(crate) visibility: Visibility,
}

impl Child {
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
        self.next = self.tree.records.subtree_end()[self.next as usize];
        Some(Child { id, visibility })
    }
}

pub(crate) struct TreeItems<'a> {
    shapes_col: &'a [Span],
    layouts: &'a [LayoutCore],
    ends: &'a [u32],
    shapes: &'a [ShapeRecord],
    cursor: usize,
    parent_end: usize,
    next_child_id: u32,
    subtree_end: u32,
}

impl<'a> TreeItems<'a> {
    pub(crate) fn new(
        records: &'a Soa<NodeRecord>,
        shapes: &'a [ShapeRecord],
        node: NodeId,
    ) -> Self {
        let shapes_col = records.shape_span();
        let parent = shapes_col[node.index()];
        Self {
            shapes_col,
            layouts: records.layout(),
            ends: records.subtree_end(),
            shapes,
            cursor: parent.start as usize,
            parent_end: (parent.start + parent.len) as usize,
            next_child_id: node.0 + 1,
            subtree_end: records.subtree_end()[node.index()],
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
                return Some(TreeItem::ShapeRecord(s));
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
            return Some(TreeItem::ShapeRecord(s));
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

#[cfg(test)]
mod tests;
