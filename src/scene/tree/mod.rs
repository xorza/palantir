//! Per-layer arena tree: SoA `records` column, sparse side tables
//! (`bounds`/`panel`/`chrome`), flat shape buffer,
//! and the subtree-rollup hashes used by cross-frame caches.
//!
//! ## Noop filtering at this tier
//!
//! Tier 2 of the pipeline's noop policy — see
//! [`paint_sink`](crate::renderer::frontend::paint_sink) for the policy
//! itself and why the tiers aren't redundant. Storage answers "is this
//! worth keeping around?", an optimization; correctness is tier 3's.
//! Two sites enforce it here, each single-site for its column:
//!
//! - `Shapes::add` drops shapes whose authoring inputs would emit no
//!   pixels (`Shape::is_noop` covers every variant). Saves per-shape
//!   lowering — payload staging, bbox math, mesh hashing — that runs
//!   inside `Shapes::add` itself.
//! - `Tree::open_node` drops a node's chrome entry from
//!   `chrome_table` when `Background::is_noop` (all of fill, stroke,
//!   shadow are no-op). Saves a slot write and keeps chrome iteration
//!   tight.
//!
//! Partial-noop chrome (e.g. shadow-only) survives storage and is
//! dropped per-emit downstream, which is why `Ui::add_shape` and the
//! encoder branches stay gate-free pass-throughs.

use crate::ClipMode;
use crate::common::content_hash::ContentHash;
use crate::common::hash::Hasher;
use crate::common::index16::Index16;
use crate::layout::scrollbars::ScrollbarsDef;
use crate::layout::types::layout_mode::{GridDefId, LayoutMode, ScrollbarsDefId};
use crate::layout::types::track::{GridDef, Track};
use crate::primitives::background::Background;
use crate::primitives::rect::Rect;
use crate::primitives::spacing::Spacing;
use crate::primitives::span::Span;
use crate::primitives::translate_scale::TranslateScale;
use crate::primitives::widget_id::WidgetId;
use crate::scene::node::Node;
use crate::scene::node::bounds_extras::BoundsExtras;
use crate::scene::node::panel_extras::PanelExtras;
use crate::scene::record_store::RecordStore;
use crate::scene::shapes::Shapes;
use crate::scene::shapes::lower;
use crate::scene::shapes::paint::ChromeRow;
use crate::scene::shapes::record::ShapeRecord;
use crate::scene::tree::extras_idx::ExtrasIdx;
use crate::scene::tree::iter::{Child, ChildIter, TreeItem, TreeItems};
use crate::scene::tree::node_id::NodeId;
use crate::scene::tree::node_record::NodeRecord;
use crate::scene::tree::paint_anims::PaintAnims;
use crate::scene::tree::recording_scratch::{OpenFrame, RecordingScratch};
use crate::scene::tree::root_slot::RootSlot;
use crate::scene::tree::subtree_end::SubtreeEnd;
use crate::scene::tree::subtree_rollups::SubtreeRollups;
use crate::scene::tree::tree_fingerprint::TreeFingerprint;
use fixedbitset::FixedBitSet;
use soa_rs::Soa;
use std::hash::{Hash, Hasher as _};

/// A single layer's arena. Per-layer trees live on
/// [`crate::scene::forest::Forest`] and share no record/shape storage,
/// so a mid-recording `Ui::layer` call dispatches straight into the
/// destination tree without interleaving — no post-record reorder pass
/// is needed to separate the layers again.
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
#[derive(Debug, Default)]
pub(crate) struct Tree {
    pub(crate) records: Soa<NodeRecord>,

    pub(crate) bounds_table: Vec<BoundsExtras>,
    pub(crate) panel_table: Vec<PanelExtras>,
    /// One row per node with chrome OR with `ClipMode::Rounded` —
    /// the rounded-clip case keeps a row even when the paint itself
    /// is fully no-op (`Background::is_noop`), so the encoder can
    /// read `bg.radius` for the stencil-mask path without a separate
    /// clip-radius column. Per-emit gates in `PaintSink::draw_*`
    /// drop the visual no-op slices; the radius survives.
    pub(crate) chrome_table: Vec<ChromeRow>,

    /// Flat per-frame shape buffer. Records are indexed via
    /// `NodeRecord.shape_span`; variable-length payloads (mesh
    /// verts/indices, polyline points/colors, gradients) live on the
    /// `RecordStore`.
    pub(crate) shapes: Shapes,

    pub(crate) grid_tracks: Vec<Track>,
    pub(crate) grid_defs: Vec<GridDef>,
    /// Side table for [`LayoutMode::Scrollbars`], same arrangement as
    /// `grid_defs`: the def is too wide for the packed 16-bit payload.
    pub(crate) scrollbar_defs: Vec<ScrollbarsDef>,

    /// Top-level root slots in this tree, in record order. Each slot's
    /// `first_node` indexes `records`; pipeline passes iterate the
    /// slice. Empty when no nodes were recorded into this tree this
    /// frame.
    pub(crate) roots: Vec<RootSlot>,

    /// Sparse shape-keyed paint animation registrations, cleared in
    /// `pre_record`. Stateless: sampling is a pure function of
    /// `Duration now` at encode time, so no per-entry timestamp is stored.
    /// See [`PaintAnims`].
    pub(crate) paint_anims: PaintAnims,

    pub(crate) rollups: SubtreeRollups,
    /// Whole-tree fingerprints, stamped alongside the per-node columns.
    pub(crate) fingerprint: TreeFingerprint,
    /// Non-leaf owners of direct text shapes. Layout iterates the set
    /// after arrange to shape paint-only text against its final padded
    /// width — a worklist, not a hash, which is why it sits here rather
    /// than with the rollup columns.
    pub(crate) container_text: FixedBitSet,
}

/// The chrome half of a [`Tree::open_node`] call: the background to
/// lower, and the store its gradient and text payloads land in. One
/// parameter rather than two because a node either has chrome and both,
/// or has neither.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ChromeInput<'a> {
    pub(crate) bg: &'a Background,
    pub(crate) store: &'a RecordStore,
}

impl Tree {
    /// Exclusive pre-order end for node `i`, grid flag stripped.
    ///
    /// `usize` rather than the stored `u32`: every caller indexes or spans
    /// with the result, so the cast belongs here once instead of at each
    /// of them. The one that stores it again narrows explicitly.
    #[inline]
    pub(crate) fn subtree_end_of(&self, i: usize) -> usize {
        self.records.subtree_end()[i].end() as usize
    }

    /// `true` iff the subtree rooted at `i` (inclusive) contains any
    /// `LayoutMode::Grid` node. Populated incrementally by `close_node`.
    #[inline]
    pub(crate) fn subtree_has_grid(&self, i: usize) -> bool {
        self.records.subtree_end()[i].has_grid()
    }

    pub(crate) fn pre_record(&mut self) {
        self.records.clear();
        self.bounds_table.clear();
        self.panel_table.clear();
        self.chrome_table.clear();
        self.shapes.clear();
        self.paint_anims.clear();
        self.grid_tracks.clear();
        self.grid_defs.clear();
        self.scrollbar_defs.clear();
        self.roots.clear();
    }

    /// Finalize this tree: populate the hash columns and derived owner sets.
    /// Capacity retained across frames. The paint-anim wake fold lives
    /// on [`crate::scene::forest::Forest::min_paint_anim_wake`] — `Ui::frame`
    /// calls it at the tail of every frame (both record + paint-only
    /// paths) so the scheduling is centralised.
    pub(crate) fn post_record(&mut self) {
        debug_assert_eq!(
            self.paint_anims.shape_indices.len(),
            self.paint_anims.entries.len(),
            "paint animation columns differ in length",
        );
        debug_assert!(
            self.paint_anims
                .shape_indices
                .last()
                .is_none_or(|&idx| idx < self.shapes.records.len() as u32),
            "paint animation shape index exceeds shapes.records",
        );
        let n = self.records.len();
        self.rollups.reset_for(n);
        // `clear` keeps the length, so the grow only does work the first
        // time the tree reaches this size. Sized here rather than at the
        // insert site so `compute_rollups`' loop carries no sizing call.
        self.container_text.clear();
        self.container_text.grow(n);
        self.fingerprint.paint_cardinality =
            paint_cardinality(self.shapes.records.len(), self.chrome_table.len(), n);
        self.compute_rollups();
    }

    /// Fused reverse-pre-order pass: computes both hash columns and
    /// discovers non-leaf direct-text owners in a single sweep.
    /// `subtree[i]` reads `node[i]` (just written this iteration) and
    /// the already-finalized `subtree[children]` (visited earlier in
    /// the reverse pass).
    fn compute_rollups(&mut self) {
        let n = self.records.len();
        let layouts = self.records.layout();
        let attrs = self.records.attrs();
        // Per-shape hashes are canonical — populated by `Shapes::add`
        // at lowering time. compute_rollups just folds them into the
        // owner's node hasher in record order.
        let shape_hashes = self.shapes.hashes.as_slice();
        let widget_ids = self.records.widget_id();
        let subtree_ends = self.records.subtree_end();
        let extras = self.records.extras();
        let bounds_tab = self.bounds_table.as_slice();
        let panel_tab = self.panel_table.as_slice();
        let chrome_tab = self.chrome_table.as_slice();
        let grid_tracks = &self.grid_tracks;
        let grid_defs = &self.grid_defs;
        let scrollbar_defs = &self.scrollbar_defs;
        let SubtreeRollups { node, subtree } = &mut self.rollups;
        let container_text = &mut self.container_text;
        // `paint_cardinality` is stamped by `post_record` before this
        // pass — a whole-tree count, not a per-node fold.
        let cascade_static = &mut self.fingerprint.cascade_static;
        let node_out = node.as_mut_slice();
        let subtree_out = subtree.as_mut_slice();
        let mut cascade_static_hasher = Hasher::new();

        for i in (0..n).rev() {
            let mut h = Hasher::new();
            layouts[i].hash_with_flags(attrs[i], &mut h);
            let ex = extras[i];
            if let Some(s) = ex.bounds {
                bounds_tab[s.idx()].hash(&mut h);
            }
            if let Some(s) = ex.panel {
                // `PanelExtras::hash` already folds `transform`
                // (identity-filtered), which is required so a
                // self-transform shift dirties `node_hash` — direct
                // shapes paint inside the transform per the
                // `Panel::transform` contract. Pinned by
                // `self_transform_change_flips_node_hash`.
                panel_tab[s.idx()].hash(&mut h);
            }
            cascade_static_hasher.write_u64(widget_ids[i].0);
            cascade_static_hasher.write_u64(h.finish());
            // Nesting, folded in so this hash actually describes the
            // tree's *shape* and not just its nodes. Without it two
            // trees with the same node count and the same per-node
            // hashes but different nesting collide, and `can_update`
            // had to zip the whole `subtree_ends` column every cascade
            // run to notice — an O(nodes) walk per layer per frame on
            // the incremental fast path, and the reason
            // `Cascade::subtree_ends` could not be the sparse
            // random-access column its doc describes.
            cascade_static_hasher.write_u32(subtree_ends[i].end());
            // Chrome authoring hash is pre-computed at lowering time
            // (`shapes::lower::background`) and stored inline on
            // `ChromeRow.hash`. Both arms write a 1-byte discriminant
            // before any payload so a chromeless node's stream can't
            // collide with a chromed node whose hash happens to start
            // `0x00`.
            if let Some(s) = ex.chrome {
                h.write_u8(1);
                h.write_u64(chrome_tab[s.idx()].hash.0);
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
            let mut has_direct_text = false;
            for item in TreeItems::new(&self.records, &self.shapes.records, NodeId(i as u32)) {
                match item {
                    TreeItem::ShapeRecord(idx, shape) => {
                        h.write_u64(shape_hashes[idx as usize].0);
                        has_direct_text |= matches!(shape, ShapeRecord::Text { .. });
                    }
                    TreeItem::Child(c) => {
                        h.write_u8(0xFF);
                        h.write_u64(widget_ids[c.id.idx()].0);
                        sh.write_u64(subtree_out[c.id.idx()].0);
                        has_children = true;
                    }
                }
            }
            // One decode for all three consumers below.
            let mode = LayoutMode::from(layouts[i].meta);
            if has_direct_text && mode != LayoutMode::Leaf {
                container_text.insert(i);
            }
            match mode {
                LayoutMode::Grid(id) => grid_defs[usize::from(id)].hash_visual(grid_tracks, &mut h),
                LayoutMode::Scrollbars(id) => scrollbar_defs[usize::from(id)].hash_visual(&mut h),
                _ => {}
            }
            let node_hash = h.finish();
            node_out[i] = ContentHash(node_hash);

            // Childless subtree = the node alone, so `node_hash` IS the
            // rollup — skip the second hasher round-trip (most nodes).
            // Inner nodes fold children (streamed above) then self.
            subtree_out[i] = if has_children {
                sh.write_u64(node_hash);
                ContentHash(sh.finish())
            } else {
                ContentHash(node_hash)
            };
        }
        *cascade_static = ContentHash(cascade_static_hasher.finish());
    }

    /// Intern one bar overlay's def, returning the id its node packs.
    /// Called only by `Scroll::show`, which is why the `open_node`
    /// debug assert can treat a dangling id as a caller bug.
    pub(crate) fn push_scrollbars_def(&mut self, def: ScrollbarsDef) -> ScrollbarsDefId {
        let id = ScrollbarsDefId::from_index(self.scrollbar_defs.len());
        self.scrollbar_defs.push(def);
        id
    }

    pub(crate) fn push_grid_def(
        &mut self,
        rows: &[Track],
        cols: &[Track],
        row_gap: f32,
        col_gap: f32,
    ) -> GridDefId {
        let id = GridDefId::from_index(self.grid_defs.len());
        self.grid_tracks.reserve(rows.len() + cols.len());
        let row_start = self.grid_tracks.len();
        self.grid_tracks.extend_from_slice(rows);
        let col_start = self.grid_tracks.len();
        self.grid_tracks.extend_from_slice(cols);
        self.grid_defs.push(GridDef {
            rows: Span::from(row_start..col_start),
            cols: Span::from(col_start..self.grid_tracks.len()),
            row_gap,
            col_gap,
        });
        id
    }

    /// Push a node as a child of the currently-open node (or as a new
    /// root if `scratch.open_frames` is empty) and make it the new tip.
    /// Root mints stamp `scratch.pending_placement` onto the new
    /// `RootSlot`; child opens don't read it. The assigned `NodeId` is
    /// the return value — the tree is the sole id authority.
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
    pub(super) fn open_node(
        &mut self,
        scratch: &mut RecordingScratch,
        widget_id: WidgetId,
        node: Node,
        chrome: Option<ChromeInput<'_>>,
    ) -> NodeId {
        // Overflow guard lives in `SubtreeEnd::new_open` (the 31-bit
        // arena ceiling), which asserts for this same id below.
        let new_id = NodeId(self.records.len() as u32);

        let parent_frame = scratch.open_frames.last().copied();

        if parent_frame.is_none() {
            let pending = scratch.pending_placement.unwrap_or_default();
            self.roots.push(RootSlot {
                first_node: new_id,
                placement: pending,
            });
        }
        let mut cols = node.into_columns(widget_id);
        // A rounded clip with no radius to round is a plain scissor.
        // Applied to the recorded flags rather than to the node, because
        // this is the only hop that sees both the node's request and the
        // chrome supplying the radius.
        if cols.attrs.clip_mode() == ClipMode::Rounded
            && chrome.is_none_or(|c| c.bg.corners.approx_zero())
        {
            cols.attrs.set_clip(ClipMode::Rect);
        }
        // Decoded once — the def-handle asserts and the self-Grid stamp
        // below all want it, and `meta` is immutable past `into_columns`
        // (only `padding` is rewritten, by the stroke inflation).
        let mode = LayoutMode::from(cols.layout.meta);
        match mode {
            LayoutMode::Grid(id) => debug_assert!(
                usize::from(id) < self.grid_defs.len(),
                "LayoutMode::Grid id {id:?} references no grid_def — only Grid::show should push grid nodes",
            ),
            LayoutMode::Scrollbars(id) => debug_assert!(
                usize::from(id) < self.scrollbar_defs.len(),
                "LayoutMode::Scrollbars id {id:?} references no scrollbar_def — only Scroll::show should push bar overlays",
            ),
            _ => {}
        }
        #[cfg(debug_assertions)]
        self.check_grid_cell(parent_frame.map(|f| f.node), &cols.bounds);

        let mut ex = ExtrasIdx::default();
        if !cols.bounds.is_default() {
            ex.bounds = Some(Index16::new(self.bounds_table.len(), "bounds_table"));
            self.bounds_table.push(cols.bounds);
        }
        if !cols.panel.is_default() {
            ex.panel = Some(Index16::new(self.panel_table.len(), "panel_table"));
            self.panel_table.push(cols.panel);
        }
        if let Some(ChromeInput { bg, store }) = chrome {
            // Chrome stroke paints fully inside the node's arranged
            // rect (see `quad.wgsl` SDF stroke band), so `padding` grows
            // by the ring on every side and children sit inside the
            // stroke without the user having to add it by hand.
            // Done here (not in the layout pass) so the layout columns
            // already carry the effective padding — zero hot-path cost
            // and the LayoutCore hash invalidates `MeasureCache`
            // automatically when the inflated value shifts.
            let ring = bg.stroke.ring();
            if ring != 0.0 {
                let [l, t, r, b] = cols.layout.padding.as_array();
                cols.layout.padding = Spacing::new(l + ring, t + ring, r + ring, b + ring);
            }
            // Tree-storage noop gate for chrome — mirrors `Shapes::add`
            // for the shape buffer and `PaintSink::draw_*` for emits.
            let needs_chrome_row =
                !bg.is_noop() || matches!(cols.attrs.clip_mode(), ClipMode::Rounded);
            if needs_chrome_row {
                let row = lower::background(store, bg);
                ex.chrome = Some(Index16::new(self.chrome_table.len(), "chrome_table"));
                self.chrome_table.push(row);
            }
        }
        // Stamp the self-Grid bit at open time — the mode is already
        // decoded above. Lets `close_node` drop its `layout[i].meta` read
        // (3 record columns → 2). `new_open` asserts the 31-bit arena
        // ceiling (high bit is the grid flag).
        let init_end = SubtreeEnd::new_open(new_id.0, matches!(mode, LayoutMode::Grid(_)));
        self.records.push(NodeRecord {
            widget_id: cols.widget_id,
            shape_span: Span::new(self.shapes.records.len() as u32, 0),
            subtree_end: init_end,
            layout: cols.layout,
            attrs: cols.attrs,
            extras: ex,
        });
        let ancestor_or_self_disabled =
            parent_frame.is_some_and(|f| f.ancestor_or_self_disabled) || cols.attrs.is_disabled();
        let effectively_visible = parent_frame.is_none_or(|f| f.effectively_visible)
            && cols.layout.meta.visibility().is_visible();
        // This child contributes one marker row to the parent's paint
        // span; the child's own counter starts past its chrome row.
        if let Some(parent) = scratch.open_frames.last_mut() {
            parent.paint_rows += 1;
        }
        scratch.open_frames.push(OpenFrame {
            node: new_id,
            ancestor_or_self_disabled,
            effectively_visible,
            paint_rows: u32::from(ex.chrome.is_some()),
        });
        new_id
    }

    /// Range-check a child's `grid` cell against its parent's
    /// `GridDef` row/col counts. Only fires when the parent is a
    /// `Grid` node and the def has nonzero rows + cols.
    ///
    /// Gated whole, not just its assert: everything it does — the load of
    /// the parent's `LayoutCore`, the `LayoutMode` decode and its
    /// `Index16` expect, the `grid_defs` index — exists to reach that
    /// assert, and this runs on every node open, the hottest step of the
    /// recording pass.
    #[cfg(debug_assertions)]
    fn check_grid_cell(&self, parent: Option<NodeId>, bounds: &BoundsExtras) {
        if let Some(parent_id) = parent {
            let parent_layout = self.records.layout()[parent_id.0 as usize];
            let LayoutMode::Grid(grid_def_id) = LayoutMode::from(parent_layout.meta) else {
                return;
            };
            let def = &self.grid_defs[usize::from(grid_def_id)];
            let n_rows = def.rows.len as usize;
            let n_cols = def.cols.len as usize;
            if n_rows > 0 && n_cols > 0 {
                let c = bounds.grid;
                let row = c.row as usize;
                let col = c.col as usize;
                let row_span = c.row_span as usize;
                let col_span = c.col_span as usize;
                debug_assert!(
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
        let popped = scratch
            .open_frames
            .pop()
            .expect("close_node called with no open node");
        let closing = popped.node;

        let i = closing.idx();
        let shapes_len = self.shapes.records.len() as u32;
        let shapes = &mut self.records.shape_span_mut()[i];
        shapes.len = shapes_len - shapes.start;

        // The paint-row stream is derived twice — counted here as the
        // record pass runs (`OpenFrame::paint_rows`, which is what
        // `PaintAnimEntry::row` indexes by), and re-derived at cascade
        // time by `compute_paint_rect` walking `TreeItems`. Nothing
        // structural ties the two, so check them against each other at
        // the one point both are knowable: `shape_span.len` was stamped
        // on the line above, which is all `TreeItems` was waiting for.
        //
        // Debug-only and once per node, against the *cascade's own*
        // enumerator rather than a second hand-written count — so this
        // fails on any drift, not only on nodes that happen to carry a
        // paint anim (the pre-existing `debug_assert!` in `damage` only
        // sees those).
        debug_assert_eq!(
            popped.paint_rows,
            u32::from(self.chrome(closing).is_some())
                + TreeItems::new(&self.records, &self.shapes.records, closing).count() as u32,
            "paint-row count drifted from the cascade's row stream at node {i}",
        );

        // `subtree_end[i]` is already the finalized "subtree contains
        // Grid" answer: self-Grid was stamped at `open_node`, and
        // descendants merged their flags up via this same code at
        // close. No `layout[i].meta` read needed — drops close_node
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

    /// Read this node's raw transform. `None` for non-panel nodes (no
    /// panel row) and for panels with an identity transform. `Panel` /
    /// `Grid` are the only widgets that expose `.transform()` in the API,
    /// so transforms always live alongside panel knobs.
    ///
    /// Private: the raw form is not the one either pass may use, so
    /// [`Self::anchored_transform`] is the whole surface.
    #[inline]
    fn transform_of(&self, id: NodeId) -> Option<TranslateScale> {
        self.records.extras()[id.idx()]
            .panel
            .map(|s| self.panel_table[s.idx()].transform)
            .filter(|t| !t.is_identity())
    }

    /// This node's transform, anchored so its scale pivots about the
    /// panel's own origin instead of the layer's `(0, 0)` — the form both
    /// readers need, and the only one either may use.
    ///
    /// `rect` is the node's arranged rect, in whatever space the caller
    /// works in. The cascade composes the result into the transform
    /// descendants inherit and paint under. The encoder pushes it around
    /// the body. If either anchors for itself, a scaled panel's body
    /// drifts from the damage rect computed for it by the
    /// `min * (1 - scale)` that [`TranslateScale::anchored_at`] cancels.
    #[inline]
    pub(crate) fn anchored_transform(&self, id: NodeId, rect: Rect) -> Option<TranslateScale> {
        self.transform_of(id).map(|t| t.anchored_at(rect.min))
    }

    /// This node's bounds extras row (position / grid cell / min_size /
    /// max_size). Falls back to `&BoundsExtras::DEFAULT` for nodes that
    /// didn't customize any field. Mirrors `Tree::panel` — callers pull
    /// the field they want.
    #[inline]
    pub(crate) fn bounds(&self, id: NodeId) -> &BoundsExtras {
        self.records.extras()[id.idx()]
            .bounds
            .map_or(&BoundsExtras::DEFAULT, |s| &self.bounds_table[s.idx()])
    }

    #[inline]
    pub(crate) fn panel(&self, id: NodeId) -> &PanelExtras {
        self.records.extras()[id.idx()]
            .panel
            .map_or(&PanelExtras::DEFAULT, |s| &self.panel_table[s.idx()])
    }

    /// Chrome paint for `id`. Present whenever the node has visible
    /// paint OR `ClipMode::Rounded` (the latter keeps a row even on
    /// `Background::is_noop` so the encoder can read `bg.radius` for
    /// the stencil-mask path). Per-emit `is_noop` gates in
    /// `PaintSink::draw_*` drop the no-paint slices; the radius
    /// always survives.
    pub(crate) fn chrome(&self, id: NodeId) -> Option<&ChromeRow> {
        self.records.extras()[id.idx()]
            .chrome
            .map(|s| &self.chrome_table[s.idx()])
    }
}

/// Fold the three counts that move whenever some node's paint-row count
/// does. Free fn so the `Tree` field reads and the arithmetic sit apart.
fn paint_cardinality(shapes: usize, chrome_rows: usize, nodes: usize) -> u64 {
    let mut h = Hasher::new();
    h.write_usize(shapes);
    h.write_usize(chrome_rows);
    h.write_usize(nodes);
    h.finish()
}

pub(crate) mod extras_idx;
pub(crate) mod iter;
pub(crate) mod node_id;
pub(crate) mod node_record;
pub(crate) mod paint_anims;
pub(crate) mod recording_scratch;
pub(crate) mod root_slot;
pub(crate) mod subtree_end;
pub(crate) mod subtree_rollups;
pub(crate) mod tree_fingerprint;

#[cfg(any(test, feature = "internals"))]
pub(crate) mod internals {
    #[cfg(test)]
    use crate::scene::shapes::record::ShapeRecord;
    #[cfg(test)]
    use crate::scene::tree::node_id::NodeId;
    use crate::scene::tree::*;

    impl Tree {
        /// Direct shapes of `node`, including parent-pushed sub-rects interleaved between children.
        #[cfg(test)]
        pub(crate) fn shapes_of(&self, node: NodeId) -> impl Iterator<Item = &ShapeRecord> + '_ {
            self.tree_items(node).filter_map(|item| match item {
                TreeItem::ShapeRecord(_, s) => Some(s),
                TreeItem::Child(_) => None,
            })
        }
    }
}

#[cfg(test)]
mod tests;
