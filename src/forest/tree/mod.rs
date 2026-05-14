//! Per-layer arena tree: SoA `records` column, sparse side tables
//! (`bounds`/`panel`/`chrome`), flat shape buffer,
//! and the subtree-rollup hashes used by cross-frame caches.
//!
//! ## Noop policy
//!
//! Tree storage is the canonical gate for "is this worth keeping
//! around?" Two sites enforce it, both single-site for their column:
//!
//! - `Shapes::add` drops shapes whose authoring inputs would emit no
//!   pixels (`Shape::is_noop` covers every variant). Saves per-shape
//!   lowering — bezier flattening, polyline tessellation, mesh
//!   hashing — that runs inside `Shapes::add` itself.
//! - `Tree::open_node` drops a node's chrome entry from
//!   `chrome_table` when `Background::is_noop` (all of fill, stroke,
//!   shadow are no-op). Saves a slot write and keeps chrome iteration
//!   tight.
//!
//! Partial-noop chrome (e.g. shadow-only) survives storage and is
//! dropped per-emit by `cmd_buffer::draw_*`'s gates. Together with
//! the authoring filter at `Shapes::add` and the emit-time gates in
//! `cmd_buffer`, every layer has exactly one canonical noop site,
//! and `Ui::add_shape` / encoder branches stay gate-free pass-throughs.

use crate::ClipMode;
use crate::common::hash::Hasher;
use crate::forest::element::{
    BoundsExtras, Element, LayoutCore, LayoutMode, NodeFlags, PanelExtras, SizeClamp,
};
use crate::forest::node::NodeRecord;
use crate::forest::rollups::{NodeHash, SubtreeRollups};
use crate::forest::shapes::Shapes;
use crate::forest::shapes::record::ShapeRecord;
use crate::forest::visibility::Visibility;
use crate::layout::types::grid_cell::GridCell;
use crate::layout::types::span::Span;
use crate::primitives::background::Background;
use crate::primitives::size::Size;
use crate::primitives::transform::TranslateScale;
use crate::primitives::widget_id::WidgetId;
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

/// Shared between [`Tree::open_node`] / [`Tree::open_node_with_chrome`].
/// Threads the parent-frame + slot id from the prologue helper through
/// the variant-specific body and into the finalize helper. `parent`
/// (the parent's `NodeId`) is derived from `parent_frame` at the call
/// site that needs it — kept here as a single pre-computed source so
/// the prologue / finalize boundary doesn't have to recompute it.
#[derive(Clone, Copy)]
struct OpenNodeCtx {
    parent_frame: Option<OpenFrame>,
    new_id: NodeId,
}

impl OpenNodeCtx {
    #[inline]
    fn parent(&self) -> Option<NodeId> {
        self.parent_frame.map(|f| f.node)
    }
}

/// One root within a single layer's [`Tree`]. Multiple roots in the
/// same tree happen for popups (eater + body recorded as two
/// top-level scopes) and any future `Ui::layer` scope that opens
/// non-contiguous top-level subtrees in the same layer.
#[derive(Clone, Copy, Debug)]
pub(crate) struct RootSlot {
    pub(crate) first_node: u32,
    /// Top-left placement in screen space. `Vec2::ZERO` for `Main`;
    /// set by `Forest::push_layer` for side layers.
    pub(crate) anchor: Vec2,
    /// Caller-supplied size cap (side layers only). `None` means
    /// "fill from `anchor` to the surface bottom-right". `Some(s)`
    /// is clamped to the surface at layout time (`available =
    /// min(s, surface - anchor)`), so a too-large cap never bleeds
    /// past the viewport. Always `None` for `Main`.
    pub(crate) size: Option<Size>,
}

/// Pending anchor entry for the `pending_anchors` stack. One per live
/// `Forest::push_layer` scope; consumed by root mints inside the scope
/// and popped at `pop_layer`.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct PendingAnchor {
    pub(crate) anchor: Vec2,
    pub(crate) size: Option<Size>,
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
/// Niche-encoded dense-table slot. `u16::MAX` means "absent"; any
/// other value is an index into a `*_table` `Vec`. Constructed by
/// [`Slot::push`] off a `Vec::len()`; resolved at read time via
/// [`Slot::get`] which folds the sentinel check into an `Option<usize>`.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct Slot(u16);

impl Slot {
    pub(crate) const ABSENT: Self = Self(u16::MAX);

    /// `len`-derived constructor. Pair with `Vec::push(v)` so the
    /// resulting `Slot` indexes the entry that push wrote. Release
    /// `assert!` because silent truncation at `len ≥ u16::MAX` would
    /// collide with [`Slot::ABSENT`] and corrupt the table mapping —
    /// invariant per CLAUDE.md's "default to release assert!".
    #[inline]
    pub(crate) fn from_len(len: usize) -> Self {
        assert!(
            len < Self::ABSENT.0 as usize,
            "Slot exhausted — more than 65 535 entries in a single sparse-column frame (got {len})",
        );
        Self(len as u16)
    }

    /// `Some(idx)` if this slot points at a real entry, `None` if
    /// absent. Single sentinel-compare folded into the `Option`.
    #[inline]
    pub(crate) fn get(self) -> Option<usize> {
        (self.0 != Self::ABSENT.0).then_some(self.0 as usize)
    }

    #[inline]
    pub(crate) fn is_present(self) -> bool {
        self.0 != Self::ABSENT.0
    }
}

impl Default for Slot {
    #[inline]
    fn default() -> Self {
        Self::ABSENT
    }
}

/// Packed per-node "extras" slot index for the four side tables. One
/// 8-byte row per node lives in `Tree::extras_idx`; that single
/// contiguous push replaces what was previously four `Vec<u16>::push`
/// calls. Each field is a [`Slot`] — niche-encoded `u16::MAX` for
/// absent, otherwise a dense index into the matching `*_table` `Vec`.
///
/// Packing wins on both ends: `Tree::open_node` does one 8-byte store
/// instead of four 2-byte stores into separate `Vec<u16>`, and the
/// hash / damage walks read all four slots from the same cache line.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct ExtrasIdx {
    pub(crate) bounds: Slot,
    pub(crate) panel: Slot,
    pub(crate) chrome: Slot,
}

#[derive(Default)]
pub(crate) struct Tree {
    // -- Per-NodeId mandatory columns ------------------------------------
    pub(crate) records: Soa<NodeRecord>,

    // -- Per-NodeId packed extras idx + dense tables ---------------------
    /// One row per node; each `u16` field indexes the matching dense
    /// `*_table` `Vec` (or holds `ExtrasIdx::ABSENT`). See
    /// [`ExtrasIdx`] for the packing rationale.
    pub(crate) extras_idx: Vec<ExtrasIdx>,
    pub(crate) bounds_table: Vec<BoundsExtras>,
    pub(crate) panel_table: Vec<PanelExtras>,
    /// One row per node with chrome OR with `ClipMode::Rounded` —
    /// the rounded-clip case keeps a row even when the paint itself
    /// is fully no-op (`Background::is_noop`), so the encoder can
    /// read `bg.radius` for the stencil-mask path without a separate
    /// clip-radius column. Per-emit gates in `cmd_buffer::draw_*`
    /// drop the visual no-op slices; the radius survives.
    pub(crate) chrome_table: Vec<Background>,

    /// Parent `NodeId` per node, or [`NodeId::ROOT`] for roots. Written
    /// at `open_node` from `open_frames.last()`; lets any post-recording
    /// pass (arrange, cascade, encode, debug) ask "who's my parent?" in
    /// O(1) without a backwards `subtree_end` walk. Same lifecycle as
    /// `records`: cleared in `pre_record`, pushed in `open_node`,
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
    /// outside the `pre_record` ↔ root `close_node` window. Capacity
    /// retained.
    ///
    /// Each frame carries a precomputed `ancestor_or_self_disabled`
    /// bit: on push, OR the new node's `disabled` with the parent
    /// frame's bit. That makes `ancestor_disabled` a one-element
    /// load (read from `last()`) instead of an O(depth) walk.
    pub(crate) open_frames: Vec<OpenFrame>,

    /// Anchor + optional size cap stack. Each `Forest::push_layer`
    /// scope pushes one entry; root mints inside the scope read the
    /// top, and `Forest::pop_layer` pops on the way out. Empty on
    /// `Main` (its implicit root always paints the full surface) and
    /// outside any `push_layer` scope; in that case root mints fall
    /// through to `PendingAnchor::default()` = `(Vec2::ZERO, None)`.
    /// Stack form keeps the no-clobber invariant local: nested
    /// `push_layer` calls (which the assert in `Forest::push_layer`
    /// currently forbids, but a future relaxation might enable) save
    /// and restore correctly without depending on that assert.
    pub(crate) pending_anchors: Vec<PendingAnchor>,

    // -- Output (populated by `post_record`) -------------------------------
    pub(crate) rollups: SubtreeRollups,

    /// Per-NodeId bit: `1` iff the subtree rooted at node `i` contains
    /// any `LayoutMode::Grid` node. Fast-path skip for `MeasureCache`'s
    /// grid-hug snapshot/restore walk. Recording-time lifecycle —
    /// cleared by `pre_record`, grown by `open_node`, and propagated
    /// up by `close_node` (so finished by the time `post_record` runs).
    pub(crate) has_grid: fixedbitset::FixedBitSet,
}

impl Tree {
    pub(crate) fn pre_record(&mut self) {
        self.records.clear();
        self.extras_idx.clear();
        self.bounds_table.clear();
        self.panel_table.clear();
        self.chrome_table.clear();
        self.parents.clear();
        self.shapes.clear();
        self.grid.clear();
        self.has_grid.clear();
        self.roots.clear();
        self.open_frames.clear();
        self.pending_anchors.clear();
    }

    /// Finalize this tree: populate `rollups.node` + `rollups.subtree`.
    /// Capacity retained across frames.
    pub(crate) fn post_record(&mut self) {
        assert!(
            self.open_frames.is_empty(),
            "post_record called with {} node(s) still open — a widget builder forgot close_node",
            self.open_frames.len(),
        );
        self.rollups.reset_for(self.records.len());
        self.compute_hashes();
    }

    /// Fused reverse-pre-order pass: computes both `rollups.node[i]`
    /// and `rollups.subtree[i]` in a single sweep. `subtree[i]` reads
    /// `node[i]` (just written this iteration) and the already-finalized
    /// `subtree[children]` (visited earlier in the reverse pass).
    fn compute_hashes(&mut self) {
        let n = self.records.len();
        let layouts = self.records.layout();
        let attrs = self.records.attrs();
        let shape_spans = self.records.shape_span();
        let ends = self.records.subtree_end();
        let shape_buf = self.shapes.records.as_slice();
        let extras = self.extras_idx.as_slice();
        let bounds_tab = self.bounds_table.as_slice();
        let panel_tab = self.panel_table.as_slice();
        let chrome_tab = self.chrome_table.as_slice();
        let grid_defs = &self.grid.defs;
        let node_out = self.rollups.node.as_mut_slice();
        let subtree_out = self.rollups.subtree.as_mut_slice();
        let paints = &mut self.rollups.paints;

        for i in (0..n).rev() {
            let mut h = Hasher::new();
            layouts[i].hash(&mut h);
            attrs[i].hash(&mut h);
            let ex = extras[i];
            if let Some(s) = ex.bounds.get() {
                // `BoundsExtras::hash` excludes transform — transform
                // is folded into the subtree hash below.
                bounds_tab[s].hash(&mut h);
            }
            if let Some(s) = ex.panel.get() {
                panel_tab[s].hash(&mut h);
            }
            let chrome = ex.chrome.get().map(|s| &chrome_tab[s]);
            chrome.hash(&mut h);

            // Walk this node's direct shapes + immediate-child position
            // markers in record order.
            let mut has_direct_shape = false;
            let parent_span = shape_spans[i];
            let parent_end = (parent_span.start + parent_span.len) as usize;
            let mut cursor = parent_span.start as usize;
            let mut next_child = (i as u32) + 1;
            let subtree_end = ends[i];
            while next_child < subtree_end {
                let cs = shape_spans[next_child as usize];
                let cs_start = cs.start as usize;
                while cursor < cs_start {
                    has_direct_shape = true;
                    shape_buf[cursor].hash(&mut h);
                    cursor += 1;
                }
                h.write_u8(0xFF);
                cursor = cs_start + cs.len as usize;
                next_child = ends[next_child as usize];
            }
            while cursor < parent_end {
                has_direct_shape = true;
                shape_buf[cursor].hash(&mut h);
                cursor += 1;
            }
            if ex.chrome.is_present() || has_direct_shape {
                paints.set(i, true);
            }
            if let LayoutMode::Grid(idx) = layouts[i].mode {
                grid_defs[idx as usize].hash(&mut h);
            }
            let node_hash = h.finish();
            node_out[i] = NodeHash(node_hash);

            // Subtree hash: seeded from `node_hash`, then folds the
            // transform (kept out of `BoundsExtras::hash` so a parent
            // moving doesn't dirty-flag children's node hashes) and
            // each direct child's already-computed `subtree[child]`.
            let mut sh = Hasher::new();
            sh.write_u64(node_hash);
            let xf = ex.panel.get().and_then(|s| panel_tab[s].transform);
            if let Some(t) = xf {
                sh.write_u8(1);
                sh.pod(&t);
            } else {
                sh.write_u8(0);
            }
            let mut next = (i as u32) + 1;
            while next < subtree_end {
                sh.write_u64(subtree_out[next as usize].0);
                next = ends[next as usize];
            }
            subtree_out[i] = NodeHash(sh.finish());
        }
    }

    /// `NodeId` the next [`Self::open_node`] call will assign — i.e.
    /// `records.len() as u32` wrapped. Lets callers (notably
    /// `Forest::open_node`) reserve the id ahead of the push so
    /// `SeenIds::record` can stash it for collision lookup before
    /// `element` is moved into the tree.
    pub(crate) fn peek_next_id(&self) -> NodeId {
        NodeId(self.records.len() as u32)
    }

    /// Push a node as a child of the currently-open node (or as a new
    /// root if `open_frames` is empty) and make it the new tip. Root
    /// mints stamp the top of `pending_anchors` onto the new
    /// `RootSlot`; child opens don't read the stack.
    ///
    /// **No-chrome variant.** `ClipMode::Rounded` always downgrades to
    /// `Rect` here — without a chrome radius there's nothing to mask.
    /// See [`Self::open_node_with_chrome`] for the chrome path.
    pub(crate) fn open_node(&mut self, mut element: Element) -> NodeId {
        if matches!(element.clip, ClipMode::Rounded) {
            element.clip = ClipMode::Rect;
        }
        let ctx = self.open_node_prologue(element.mode);
        let cols = element.into_columns();
        self.check_grid_cell(ctx.parent(), &cols.bounds);

        let mut ex = ExtrasIdx::default();
        if !cols.bounds.is_default() {
            ex.bounds = Slot::from_len(self.bounds_table.len());
            self.bounds_table.push(cols.bounds);
        }
        if !cols.panel.is_default() {
            ex.panel = Slot::from_len(self.panel_table.len());
            self.panel_table.push(cols.panel);
        }
        self.extras_idx.push(ex);
        self.open_node_finalize(ctx, cols.widget_id, cols.layout, cols.attrs)
    }

    /// Chrome variant of [`Self::open_node`]. Pushes the `Background`
    /// row into `chrome_table`. Split from the no-chrome path so neither
    /// call site carries the 232-byte `Option<Background>` parameter,
    /// and so the `ClipMode::Rounded` zero-radius downgrade can be
    /// skipped statically when no radius is present.
    pub(crate) fn open_node_with_chrome(&mut self, mut element: Element, bg: Background) -> NodeId {
        // Tree-storage noop gate for chrome — mirrors `Shapes::add` for
        // the shape buffer and `cmd_buffer::draw_*` for emits. Whole-
        // `Background::is_noop` drops the entry so chrome iteration /
        // hashing skips it. Partial-noop chrome (e.g. shadow-only)
        // survives here and is dropped per-emit by the cmd buffer's
        // gates. When `ClipMode::Rounded`, the chrome row is also
        // kept past `Background::is_noop` so the encoder can read
        // `bg.radius` for the stencil-mask path — that's the only
        // time a noop chrome ever survives storage.
        if matches!(element.clip, ClipMode::Rounded) && bg.radius.approx_zero() {
            element.clip = ClipMode::Rect;
        }
        let ctx = self.open_node_prologue(element.mode);
        let cols = element.into_columns();
        self.check_grid_cell(ctx.parent(), &cols.bounds);

        let mut ex = ExtrasIdx::default();
        if !cols.bounds.is_default() {
            ex.bounds = Slot::from_len(self.bounds_table.len());
            self.bounds_table.push(cols.bounds);
        }
        if !cols.panel.is_default() {
            ex.panel = Slot::from_len(self.panel_table.len());
            self.panel_table.push(cols.panel);
        }
        let needs_chrome_row = !bg.is_noop() || matches!(cols.attrs.clip_mode(), ClipMode::Rounded);
        if needs_chrome_row {
            ex.chrome = Slot::from_len(self.chrome_table.len());
            self.chrome_table.push(bg);
        }
        self.extras_idx.push(ex);
        self.open_node_finalize(ctx, cols.widget_id, cols.layout, cols.attrs)
    }

    /// Roots/parent bookkeeping shared by both `open_node` variants.
    /// Captures the parent frame + slot id so the body can drive the
    /// chrome-specific table writes, then returns to
    /// [`Self::open_node_finalize`].
    #[inline(always)]
    fn open_node_prologue(&mut self, mode: LayoutMode) -> OpenNodeCtx {
        let parent_frame = self.open_frames.last().copied();
        let new_id = self.peek_next_id();
        if parent_frame.is_none() {
            let pending = self.pending_anchors.last().copied().unwrap_or_default();
            self.roots.push(RootSlot {
                first_node: new_id.0,
                anchor: pending.anchor,
                size: pending.size,
            });
        }
        if let LayoutMode::Grid(idx) = mode {
            assert!(
                (idx as usize) < self.grid.defs.len(),
                "LayoutMode::Grid({idx}) references no grid_def — only Grid::show should push grid nodes",
            );
        }
        OpenNodeCtx {
            parent_frame,
            new_id,
        }
    }

    /// Range-check a child's `grid` cell against its parent's
    /// `GridDef` row/col counts. Only fires when the parent is a
    /// `Grid` node and the def has nonzero rows + cols.
    #[inline(always)]
    fn check_grid_cell(&self, parent: Option<NodeId>, bounds: &BoundsExtras) {
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
    }

    /// Records-column push + `open_frames` push. Returns the new node
    /// id so the caller's `?`-style flow stays linear.
    #[inline(always)]
    fn open_node_finalize(
        &mut self,
        ctx: OpenNodeCtx,
        widget_id: WidgetId,
        layout: LayoutCore,
        attrs: NodeFlags,
    ) -> NodeId {
        self.records.push(NodeRecord {
            widget_id,
            shape_span: Span::new(self.shapes.records.len() as u32, 0),
            subtree_end: ctx.new_id.0 + 1,
            layout,
            attrs,
        });
        self.parents.push(ctx.parent().unwrap_or(NodeId::ROOT));
        self.has_grid.grow(self.records.len());
        // Column length-equality. `records` + `extras_idx` + `parents`
        // must agree on `len`; a missed push silently shifts every
        // later node's index. Invariant is structurally guarded by the
        // unconditional pushes above — debug-only check.
        #[cfg(debug_assertions)]
        {
            let n = self.records.len();
            assert_eq!(self.extras_idx.len(), n);
            assert_eq!(self.parents.len(), n);
        }
        let ancestor_or_self_disabled = ctx
            .parent_frame
            .is_some_and(|f| f.ancestor_or_self_disabled)
            || attrs.is_disabled();
        self.open_frames.push(OpenFrame {
            node: ctx.new_id,
            ancestor_or_self_disabled,
        });
        ctx.new_id
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
            self.has_grid.insert(i);
        }
        let i_has_grid = self.has_grid.contains(i);

        if let Some(parent) = self.open_frames.last().map(|f| f.node) {
            let pi = parent.index();
            let ends = self.records.subtree_end_mut();
            if ends[pi] < end {
                ends[pi] = end;
            }
            if i_has_grid {
                self.has_grid.insert(pi);
            }
        }
    }

    /// Iterate children of `parent` in declaration order, each tagged
    /// with its collapse state. Use [`Tree::active_children`] when you
    /// only need non-collapsed children — that's the dominant access
    /// pattern.
    pub(crate) fn children(&self, parent: NodeId) -> ChildIter<'_> {
        let pi = parent.0 as usize;
        ChildIter {
            layouts: self.records.layout(),
            ends: self.records.subtree_end(),
            next: parent.0 + 1,
            end: self.records.subtree_end()[pi],
        }
    }

    /// Iterate non-collapsed children of `parent`, yielding `NodeId`s
    /// directly. Equivalent to `children(parent).filter_map(Child::active)`
    /// but shorter at call sites — most layout drivers want this form.
    pub(crate) fn active_children(&self, parent: NodeId) -> impl Iterator<Item = NodeId> + '_ {
        self.children(parent).filter_map(Child::active)
    }

    pub(crate) fn tree_items(&self, node: NodeId) -> TreeItems<'_> {
        TreeItems::new(&self.records, &self.shapes.records, node)
    }

    /// Read this node's transform. Returns `None` for non-panel nodes
    /// (no panel row) and for panels without a transform set. `Panel`
    /// / `Grid` are the only widgets that expose `.transform()` in the
    /// API, so transforms always live alongside panel knobs.
    #[inline]
    pub(crate) fn transform_of(&self, id: NodeId) -> Option<TranslateScale> {
        self.extras_idx[id.index()]
            .panel
            .get()
            .and_then(|s| self.panel_table[s].transform)
    }

    #[inline]
    pub(crate) fn position_of(&self, id: NodeId) -> Vec2 {
        self.extras_idx[id.index()]
            .bounds
            .get()
            .map_or(Vec2::ZERO, |s| self.bounds_table[s].position)
    }

    #[inline]
    pub(crate) fn grid_of(&self, id: NodeId) -> GridCell {
        self.extras_idx[id.index()]
            .bounds
            .get()
            .map_or(BoundsExtras::DEFAULT.grid, |s| self.bounds_table[s].grid)
    }

    /// Paired read of `(min_size, max_size)` — they're always read
    /// together by `layoutengine` / `intrinsic` / `stack`.
    #[inline]
    pub(crate) fn size_clamps_of(&self, id: NodeId) -> SizeClamp {
        match self.extras_idx[id.index()].bounds.get() {
            Some(s) => {
                let b = &self.bounds_table[s];
                SizeClamp {
                    min: b.min_size,
                    max: b.max_size,
                }
            }
            None => SizeClamp {
                min: BoundsExtras::DEFAULT.min_size,
                max: BoundsExtras::DEFAULT.max_size,
            },
        }
    }

    #[inline]
    pub(crate) fn panel(&self, id: NodeId) -> &PanelExtras {
        self.extras_idx[id.index()]
            .panel
            .get()
            .map_or(&PanelExtras::DEFAULT, |s| &self.panel_table[s])
    }

    /// Chrome paint for `id`. Present whenever the node has visible
    /// paint OR `ClipMode::Rounded` (the latter keeps a row even on
    /// `Background::is_noop` so the encoder can read `bg.radius` for
    /// the stencil-mask path). Per-emit `is_noop` gates in
    /// `cmd_buffer::draw_*` drop the no-paint slices; the radius
    /// always survives.
    pub(crate) fn chrome(&self, id: NodeId) -> Option<&Background> {
        self.extras_idx[id.index()]
            .chrome
            .get()
            .map(|s| &self.chrome_table[s])
    }
}

pub(crate) struct ChildIter<'a> {
    layouts: &'a [LayoutCore],
    ends: &'a [u32],
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
        let i = self.next as usize;
        let visibility = self.layouts[i].visibility;
        self.next = self.ends[i];
        Some(Child {
            id: NodeId(i as u32),
            visibility,
        })
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
/// `Layout` since the tree is read-only after recording.
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
