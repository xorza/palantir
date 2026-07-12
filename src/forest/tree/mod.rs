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
use crate::forest::Chrome;
use crate::forest::element::{BoundsExtras, Element, LayoutMode, PanelExtras};
use crate::forest::node::{NodeRecord, SubtreeEnd};
use crate::forest::rollups::{NodeHash, SubtreeRollups};
use crate::forest::shapes::Shapes;
use crate::forest::shapes::record::ChromeRow;
use crate::forest::tree::iter::{Child, ChildIter, TreeItem, TreeItems};
use crate::forest::tree::paint_anims::PaintAnims;
use crate::forest::tree::record::{OpenFrame, RecordingScratch, RootSlot};
use crate::layout::types::track::GridDef;
use crate::primitives::approx::noop_f32;
use crate::primitives::span::Span;
use crate::primitives::transform::TranslateScale;
use crate::primitives::widget_id::WidgetId;
use soa_rs::Soa;
use std::hash::{Hash, Hasher as _};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct NodeId(pub(crate) u32);

impl NodeId {
    #[inline]
    pub(crate) fn idx(self) -> usize {
        self.0 as usize
    }
}

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
            "Slot exhausted — {} entries fill the sparse-column frame; index would collide with Slot::ABSENT (got {len})",
            Self::ABSENT.0 as usize,
        );
        Self(len as u16)
    }

    /// `Some(idx)` if this slot points at a real entry, `None` if
    /// absent. Single sentinel-compare folded into the `Option`.
    #[inline]
    pub(crate) fn get(self) -> Option<usize> {
        (self.0 != Self::ABSENT.0).then_some(self.0 as usize)
    }
}

impl Default for Slot {
    #[inline]
    fn default() -> Self {
        Self::ABSENT
    }
}

/// Packed per-node "extras" slot index for the side tables. One 6-byte
/// row per node lives in `Tree::extras_idx`; that single contiguous
/// push replaces what was previously three `Vec<u16>::push` calls.
/// Each field is a [`Slot`] — niche-encoded `u16::MAX` for absent,
/// otherwise a dense index into the matching `*_table` `Vec`.
///
/// Packing wins on both ends: `Tree::open_node` does one packed store
/// instead of three 2-byte stores into separate `Vec<u16>`, and the
/// hash / damage walks read all three slots from the same cache line.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct ExtrasIdx {
    pub(crate) bounds: Slot,
    pub(crate) panel: Slot,
    pub(crate) chrome: Slot,
}

/// A single layer's arena. Per-layer trees live on
/// [`crate::forest::Forest`] and share no record/shape storage —
/// mid-recording `Ui::layer` calls dispatch into the destination tree
/// without interleaving, eliminating the prior reorder pass.
///
/// **`records`** is `Soa<NodeRecord>` indexed by `NodeId.0`, in pre-order
/// paint order (parent before children, siblings in declaration order).
/// Reverse iteration gives topmost-first (used by hit-testing). `soa-rs`
/// lays each `NodeRecord` field out as its own contiguous slice, so each
/// pass touches only the bytes it needs:
///
/// - `layout`      — read by measure / arrange / alignment math
/// - `attrs`       — packed paint/input flags (2 B); cascade / encoder
/// - `widget_id`   — hit-test, state map, damage diff
/// - `subtree_end` — pre-order skip + grid flag (every walk)
/// - `shape_span`  — span into the flat shape buffer covering this node's
///   subtree (parent + descendants); the gap between children's
///   sub-ranges holds the parent's direct shapes in record order.
#[derive(Default)]
pub(crate) struct Tree {
    // -- Per-NodeId mandatory columns ------------------------------------
    pub(crate) records: Soa<NodeRecord>,

    // -- Per-NodeId packed extras idx + dense tables ---------------------
    /// One row per node; each `u16` field indexes the matching dense
    /// `*_table` `Vec` (or holds `Slot::ABSENT`). See
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
    pub(crate) chrome_table: Vec<ChromeRow>,

    // -- Shapes ----------------------------------------------------------
    /// Flat per-frame shape buffer. Records are indexed via
    /// `NodeRecord.shape_span`; variable-length payloads (mesh
    /// verts/indices, polyline points/colors, gradients) live on the
    /// `FrameArena`.
    pub(crate) shapes: Shapes,

    // -- Frame-scoped sub-storage ----------------------------------------
    pub(crate) grid: GridArena,

    // -- Roots -----------------------------------------------------------
    /// Top-level root slots in this tree, in record order. Each slot's
    /// `first_node` indexes `records`; pipeline passes iterate the
    /// slice. Empty when no nodes were recorded into this tree this
    /// frame.
    pub(crate) roots: Vec<RootSlot>,

    // -- Paint-anim registry ----------------------------------------------
    /// Shape-keyed paint animation registrations. Pushed in lockstep
    /// with `shapes.records` via `Forest::add_shape{,_animated}`,
    /// cleared in `pre_record`. Stateless: sampling is a pure function
    /// of `Duration now` at encode time, so no per-entry timestamp is
    /// stored. See [`PaintAnims`] and `docs/roadmap/paint-tick.md`.
    pub(crate) paint_anims: PaintAnims,

    // -- Output (populated by `post_record`) -------------------------------
    pub(crate) rollups: SubtreeRollups,
}

impl Tree {
    /// Exclusive pre-order end for node `i`, grid flag stripped.
    #[inline]
    pub(crate) fn subtree_end_of(&self, i: usize) -> u32 {
        self.records.subtree_end()[i].end()
    }

    /// `true` iff the subtree rooted at `i` (inclusive) contains any
    /// `LayoutMode::Grid` node. Populated incrementally by `close_node`.
    #[inline]
    pub(crate) fn subtree_has_grid(&self, i: usize) -> bool {
        self.records.subtree_end()[i].has_grid()
    }

    pub(crate) fn pre_record(&mut self) {
        self.records.clear();
        self.extras_idx.clear();
        self.bounds_table.clear();
        self.panel_table.clear();
        self.chrome_table.clear();
        self.shapes.clear();
        self.paint_anims.clear();
        self.grid.clear();
        self.roots.clear();
    }

    /// Finalize this tree: populate `rollups.node` + `rollups.subtree`.
    /// Capacity retained across frames. The paint-anim wake fold lives
    /// on [`crate::forest::Forest::min_paint_anim_wake`] — `Ui::frame`
    /// calls it at the tail of every frame (both record + paint-only
    /// paths) so the scheduling is centralised.
    pub(crate) fn post_record(&mut self) {
        // `by_shape` is lazy — empty in frames with no animated
        // shapes, otherwise sized to `last_animated_shape_idx + 1`.
        // Encoder treats `idx >= by_shape.len()` as "no anim". Sanity
        // check: the table can never legitimately exceed the shape
        // buffer.
        assert!(
            self.paint_anims.by_shape.len() <= self.shapes.records.len(),
            "paint_anims.by_shape exceeds shapes.records",
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
        // Per-shape hashes are canonical — populated by `Shapes::add`
        // at lowering time. compute_hashes just folds them into the
        // owner's node hasher in record order.
        let shape_hashes = self.shapes.hashes.as_slice();
        let widget_ids = self.records.widget_id();
        let extras = self.extras_idx.as_slice();
        let bounds_tab = self.bounds_table.as_slice();
        let panel_tab = self.panel_table.as_slice();
        let chrome_tab = self.chrome_table.as_slice();
        let grid_defs = &self.grid.defs;
        let node_out = self.rollups.node.as_mut_slice();
        let subtree_out = self.rollups.subtree.as_mut_slice();

        for i in (0..n).rev() {
            let mut h = Hasher::new();
            layouts[i].hash_with_flags(attrs[i], &mut h);
            let ex = extras[i];
            if let Some(s) = ex.bounds.get() {
                bounds_tab[s].hash(&mut h);
            }
            if let Some(s) = ex.panel.get() {
                // `PanelExtras::hash` already folds `transform`
                // (identity-filtered), which is required so a
                // self-transform shift dirties `node_hash` — direct
                // shapes paint inside the transform per the
                // `Panel::transform` contract. Pinned by
                // `self_transform_change_flips_node_hash`.
                panel_tab[s].hash(&mut h);
            }
            // Chrome authoring hash is pre-computed at lowering time
            // (`FrameArena::lower_background`) and stored inline on
            // `ChromeRow.hash`. Both arms write a 1-byte discriminant
            // before any payload so a chromeless node's stream can't
            // collide with a chromed node whose hash happens to start
            // `0x00`.
            if let Some(s) = ex.chrome.get() {
                h.write_u8(1);
                h.write_u64(chrome_tab[s].hash.0);
            } else {
                h.write_u8(0);
            }

            // Walk this node's direct shapes + immediate-child position
            // markers in record order via the shared `TreeItems`
            // traversal — single source of truth for the parent/child
            // interleave cursor logic (encoder uses the same iterator).
            // Each shape's canonical hash was computed at `Shapes::add`
            // time; fold it in as a u64 so we don't re-hash the record
            // fields here. Child markers carry the child's `WidgetId`
            // (behind a `0xFF` domain separator) so `node_hash` covers
            // the full paint-order identity stream: a child↔child
            // reorder or a shape crossing a child boundary flips the
            // hash and routes the parent to the damage diff's
            // changed-paints arm, whose row matcher emits the
            // order-inversion damage. The cost is that re-keying a
            // child (same content, new id) also flips the parent
            // chain's node/subtree hashes — a one-frame MeasureCache
            // miss and a no-damage re-diff of the parent's rows —
            // accepted, since re-keys are rare and almost always ride
            // a structural change that invalidates those anyway.
            //
            // The subtree hasher rides the same walk: each child's
            // already-finalized `subtree[child]` (reverse pre-order —
            // children were visited earlier) folds in as it's yielded,
            // and `node_hash` is appended after `finish` below —
            // children-then-self, one traversal instead of a second
            // child-hop loop.
            let mut sh = Hasher::new();
            let mut has_children = false;
            for item in TreeItems::new(&self.records, &self.shapes.records, NodeId(i as u32)) {
                match item {
                    TreeItem::ShapeRecord(idx, _) => h.write_u64(shape_hashes[idx as usize].0),
                    TreeItem::Child(c) => {
                        h.write_u8(0xFF);
                        h.write_u64(widget_ids[c.id.idx()].0);
                        sh.write_u64(subtree_out[c.id.idx()].0);
                        has_children = true;
                    }
                }
            }
            if layouts[i].mode == LayoutMode::Grid {
                let idx = layouts[i].mode_payload;
                grid_defs[idx as usize].hash(&mut h);
            }
            let node_hash = h.finish();
            node_out[i] = NodeHash(node_hash);

            // Childless subtree = the node alone, so `node_hash` IS the
            // rollup — skip the second hasher round-trip (most nodes).
            // Inner nodes fold children (streamed above) then self.
            subtree_out[i] = if has_children {
                sh.write_u64(node_hash);
                NodeHash(sh.finish())
            } else {
                NodeHash(node_hash)
            };
        }
    }

    /// `NodeId` the next [`Self::open_node`] call will assign — i.e.
    /// `records.len() as u32` wrapped. Lets callers (notably
    /// `Forest::open_node`) reserve the id ahead of the push so
    /// `SeenIds::record` can stash it for collision lookup before
    /// `element` is moved into the tree.
    pub(crate) fn peek_next_id(&self) -> NodeId {
        // Overflow guard lives in `SubtreeEnd::new_open` (the 31-bit
        // arena ceiling), which `open_node` asserts for this same id.
        NodeId(self.records.len() as u32)
    }

    /// Push a node as a child of the currently-open node (or as a new
    /// root if `scratch.open_frames` is empty) and make it the new tip.
    /// Root mints stamp `scratch.pending_anchor` onto the new
    /// `RootSlot`; child opens don't read it.
    ///
    /// `new_id` is the pre-reserved id `Forest::open_node` already
    /// computed via [`Self::peek_next_id`] to build the `SeenIds`
    /// endpoint. Threading it through avoids recomputing
    /// `records.len()` twice per node.
    ///
    /// `chrome` is `None` for nodes without a background paint;
    /// `ClipMode::Rounded` always downgrades to `Rect` in that case
    /// (no radius to mask). With chrome, the row is kept past
    /// `Background::is_noop` when `ClipMode::Rounded` so the encoder
    /// can read `bg.radius` for the stencil-mask path — the only time
    /// a noop chrome survives storage. Partial-noop chrome (e.g.
    /// shadow-only) survives here and is dropped per-emit by the cmd
    /// buffer's gates.
    #[inline]
    pub(crate) fn open_node(
        &mut self,
        scratch: &mut RecordingScratch,
        new_id: NodeId,
        widget_id: WidgetId,
        mut element: Element,
        chrome: Option<Chrome<'_>>,
    ) -> NodeId {
        debug_assert_eq!(
            new_id.0 as usize,
            self.records.len(),
            "Tree::open_node received a NodeId that doesn't match the next slot",
        );

        if matches!(element.flags.clip_mode(), ClipMode::Rounded) {
            let radius_zero = chrome.is_none_or(|c| c.bg.corners.approx_zero());
            if radius_zero {
                element.flags.set_clip(ClipMode::Rect);
            }
        }

        let parent_frame = scratch.open_frames.last().copied();

        if parent_frame.is_none() {
            let pending = scratch.pending_anchor.unwrap_or_default();
            self.roots.push(RootSlot {
                first_node: new_id,
                placement: pending,
            });
        }
        if matches!(element.mode, LayoutMode::Grid) {
            assert!(
                (element.mode_payload as usize) < self.grid.defs.len(),
                "LayoutMode::Grid idx {} references no grid_def — only Grid::show should push grid nodes",
                element.mode_payload,
            );
        }

        let mut cols = element.into_columns(widget_id);
        self.check_grid_cell(parent_frame.map(|f| f.node), &cols.bounds);

        let mut ex = ExtrasIdx::default();
        if !cols.bounds.is_default() {
            ex.bounds = Slot::from_len(self.bounds_table.len());
            self.bounds_table.push(cols.bounds);
        }
        if !cols.panel.is_default() {
            ex.panel = Slot::from_len(self.panel_table.len());
            self.panel_table.push(cols.panel);
        }
        if let Some(Chrome { bg, arena, atlas }) = chrome {
            // Chrome stroke paints fully inside the node's arranged
            // rect (see `quad.wgsl` SDF stroke band). Inflate `padding`
            // by `stroke.width` on every side so children sit inside
            // the stroke without the user having to add it by hand.
            // Done here (not in the layout pass) so the layout columns
            // already carry the effective padding — zero hot-path cost
            // and the LayoutCore hash invalidates `MeasureCache`
            // automatically when the inflated value shifts.
            if !noop_f32(bg.stroke.width) {
                let s = bg.stroke.width;
                let [l, t, r, b] = cols.layout.padding.as_array();
                cols.layout.padding =
                    crate::primitives::spacing::Spacing::new(l + s, t + s, r + s, b + s);
            }
            // Tree-storage noop gate for chrome — mirrors `Shapes::add`
            // for the shape buffer and `cmd_buffer::draw_*` for emits.
            let needs_chrome_row =
                !bg.is_noop() || matches!(cols.attrs.clip_mode(), ClipMode::Rounded);
            if needs_chrome_row {
                let row = arena.lower_background(bg, atlas);
                ex.chrome = Slot::from_len(self.chrome_table.len());
                self.chrome_table.push(row);
            }
        }
        self.extras_idx.push(ex);

        // Stamp the self-Grid bit at open time — `cols.layout.mode` is
        // already in registers here. Lets `close_node` drop its
        // `layout[i].mode` read (3 record columns → 2). `new_open`
        // asserts the 31-bit arena ceiling (high bit is the grid flag).
        let init_end = SubtreeEnd::new_open(new_id.0, cols.layout.mode == LayoutMode::Grid);
        self.records.push(NodeRecord {
            widget_id: cols.widget_id,
            shape_span: Span::new(self.shapes.records.len() as u32, 0),
            subtree_end: init_end,
            layout: cols.layout,
            attrs: cols.attrs,
        });
        // Column length-equality. `records` + `extras_idx` are the two
        // per-node SoA columns and must agree on `len`; a missed push
        // silently shifts every later node's index. (The `bounds`/`panel`/
        // `chrome` tables are `Slot`-indexed and sparse, so they're not
        // 1:1 with `records`.) One integer compare per node — cheap enough
        // to keep in release per the invariant-assert convention.
        assert_eq!(self.extras_idx.len(), self.records.len());
        let ancestor_or_self_disabled =
            parent_frame.is_some_and(|f| f.ancestor_or_self_disabled) || cols.attrs.is_disabled();
        scratch.open_frames.push(OpenFrame {
            node: new_id,
            ancestor_or_self_disabled,
        });
        new_id
    }

    /// Range-check a child's `grid` cell against its parent's
    /// `GridDef` row/col counts. Only fires when the parent is a
    /// `Grid` node and the def has nonzero rows + cols.
    #[inline(always)]
    fn check_grid_cell(&self, parent: Option<NodeId>, bounds: &BoundsExtras) {
        if let Some(parent_id) = parent {
            let parent_layout = self.records.layout()[parent_id.0 as usize];
            if parent_layout.mode != LayoutMode::Grid {
                return;
            }
            let def = &self.grid.defs[parent_layout.mode_payload as usize];
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

    pub(crate) fn close_node(&mut self, scratch: &mut RecordingScratch) {
        let closing = scratch
            .open_frames
            .pop()
            .expect("close_node called with no open node")
            .node;

        let i = closing.idx();
        let shapes_len = self.shapes.records.len() as u32;
        let shapes = &mut self.records.shape_span_mut()[i];
        shapes.len = shapes_len - shapes.start;

        // `subtree_end[i]` is already the finalized "subtree contains
        // Grid" answer: self-Grid was stamped at `open_node`, and
        // descendants merged their flags up via this same code at
        // close. No `layout[i].mode` read needed — drops close_node
        // from 3 record-column touches to 2.
        let child_end = self.records.subtree_end()[i];

        if let Some(parent) = scratch.open_frames.last().map(|f| f.node) {
            let pi = parent.idx();
            self.records.subtree_end_mut()[pi].merge_child(child_end);
        }
    }

    /// Iterate children of `parent` in declaration order, each tagged
    /// with its collapse state. Use [`Tree::active_children`] when you
    /// only need non-collapsed children — that's the dominant access
    /// pattern.
    pub(crate) fn children(&self, parent: NodeId) -> ChildIter<'_> {
        let ends = self.records.subtree_end();
        ChildIter {
            layouts: self.records.layout(),
            next: parent.0 + 1,
            end: ends[parent.0 as usize].end(),
            ends,
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
    /// (no panel row) and for panels with an identity transform. `Panel`
    /// / `Grid` are the only widgets that expose `.transform()` in the
    /// API, so transforms always live alongside panel knobs.
    #[inline]
    pub(crate) fn transform_of(&self, id: NodeId) -> Option<TranslateScale> {
        self.extras_idx[id.idx()]
            .panel
            .get()
            .map(|s| self.panel_table[s].transform)
            .filter(|t| !t.is_noop())
    }

    /// This node's bounds extras row (position / grid cell / min_size /
    /// max_size). Falls back to `&BoundsExtras::DEFAULT` for nodes that
    /// didn't customize any field. Mirrors `Tree::panel` — callers pull
    /// the field they want.
    #[inline]
    pub(crate) fn bounds(&self, id: NodeId) -> &BoundsExtras {
        self.extras_idx[id.idx()]
            .bounds
            .get()
            .map_or(&BoundsExtras::DEFAULT, |s| &self.bounds_table[s])
    }

    #[inline]
    pub(crate) fn panel(&self, id: NodeId) -> &PanelExtras {
        self.extras_idx[id.idx()]
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
    pub(crate) fn chrome(&self, id: NodeId) -> Option<&ChromeRow> {
        self.extras_idx[id.idx()]
            .chrome
            .get()
            .map(|s| &self.chrome_table[s])
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

pub(crate) mod iter;
pub(crate) mod paint_anims;
pub(crate) mod record;

#[cfg(any(test, feature = "internals"))]
pub mod test_support {
    #![allow(dead_code)]
    use crate::forest::shapes::record::ShapeRecord;
    use crate::forest::tree::*;

    impl Tree {
        /// Direct shapes of `node`, including parent-pushed sub-rects interleaved between children.
        pub fn shapes_of(&self, node: NodeId) -> impl Iterator<Item = &ShapeRecord> + '_ {
            self.tree_items(node).filter_map(|item| match item {
                TreeItem::ShapeRecord(_, s) => Some(s),
                TreeItem::Child(_) => None,
            })
        }
    }
}

#[cfg(test)]
mod tests;
