//! Per-layer arena tree: SoA `records` column, sparse side tables, flat shape
//! buffer, and subtree-rollup hashes for cross-frame caches.
//!
//! Noop filtering at this storage tier: `Shapes::add` drops `Shape::is_noop`
//! shapes and `Tree::open_node` drops `Background::is_noop` chrome rows (policy in
//! [`paint_sink`](crate::renderer::frontend::paint_sink)). Partial-noop chrome is
//! dropped per emit downstream.

pub(crate) mod extras_idx;
pub(crate) mod iter;
pub(crate) mod node_id;
pub(crate) mod node_record;
pub(crate) mod paint_anims;
pub(crate) mod recording_scratch;
mod rollup_inputs;
pub(crate) mod root_slot;
pub(crate) mod subtree_end;
pub(crate) mod subtree_rollups;
pub(crate) mod tree_fingerprint;

use crate::common::content_hash::ContentHash;
use crate::common::hash::Hasher;
use crate::common::index16::Index16;
use crate::common::span::Span;
use crate::layout::drivers::scrollbars::scrollbars_def::ResolvedScrollbarsDef;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::geometry::spacing::Spacing;
use crate::primitives::geometry::translate_scale::TranslateScale;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::layout::clip_mode::ClipMode;
use crate::primitives::layout::layout_mode::{GridDefId, LayoutMode, ScrollbarsDefId};
use crate::primitives::layout::track::{GridDef, Track};
use crate::primitives::paint::background::Background;
use crate::primitives::paint::stroke::Stroke;
use crate::scene::node::Node;
use crate::scene::node::bounds_extras::BoundsExtras;
use crate::scene::node::node_flags::NodeFlags;
use crate::scene::node::panel_extras::PanelExtras;
use crate::scene::record_store::RecordStore;
use crate::scene::tree::extras_idx::ExtrasIdx;
use crate::scene::tree::iter::{Child, ChildIter, TreeItem, TreeItems};
use crate::scene::tree::node_id::NodeId;
use crate::scene::tree::node_record::NodeRecord;
use crate::scene::tree::paint_anims::PaintAnims;
use crate::scene::tree::recording_scratch::{OpenFrame, RecordingScratch};
#[cfg(debug_assertions)]
use crate::scene::tree::rollup_inputs::RollupCheck;
use crate::scene::tree::rollup_inputs::RollupInputs;
use crate::scene::tree::root_slot::RootSlot;
use crate::scene::tree::subtree_end::SubtreeEnd;
use crate::scene::tree::subtree_rollups::SubtreeRollups;
use crate::scene::tree::tree_fingerprint::TreeFingerprint;
use crate::shape::lower;
use crate::shape::paint::chrome_row::ChromeRow;
use crate::shape::paint::shape_stroke::ShapeStroke;
use crate::shape::shapes::Shapes;
use fixedbitset::FixedBitSet;
use soa_rs::Soa;
use std::hash::{Hash, Hasher as _};
use std::mem;

/// A single layer's arena. Layers share no storage, so `Ui::layer` records
/// straight into its destination tree. `records` is in pre-order paint order;
/// reverse iteration is topmost-first.
#[derive(Debug, Default)]
pub(crate) struct Tree {
    pub(crate) records: Soa<NodeRecord>,

    pub(crate) bounds_table: Vec<BoundsExtras>,
    pub(crate) panel_table: Vec<PanelExtras>,
    /// One row per node with chrome or `ClipMode::Rounded`; a rounded clip keeps its
    /// row even for a noop paint so the encoder can read `bg.radius`.
    pub(crate) chrome_table: Vec<ChromeRow>,
    /// The focus ring's stroke for the one chrome row that flags it ([`ChromeRow::ring`]).
    pub(crate) focus_ring: ShapeStroke,

    pub(crate) shapes: Shapes,

    pub(crate) grid_tracks: Vec<Track>,
    pub(crate) grid_defs: Vec<GridDef>,
    /// Side table for [`LayoutMode::Scrollbars`]; the def is too wide to pack.
    pub(crate) scrollbar_defs: Vec<ResolvedScrollbarsDef>,

    pub(crate) roots: Vec<RootSlot>,

    /// Sparse shape-keyed paint animations, cleared in `pre_record`. Sampling is a
    /// pure function of `now`, so no timestamps are stored.
    pub(crate) paint_anims: PaintAnims,

    pub(crate) rollups: SubtreeRollups,
    pub(crate) fingerprint: TreeFingerprint,
    /// Non-leaf owners of direct text shapes that paint. Layout shapes their text
    /// after arrange. Visibility is folded in at build time.
    pub(crate) container_text: FixedBitSet,
    last_inputs: RollupInputs,
    #[cfg(debug_assertions)]
    rollup_check: RollupCheck,
}

/// The chrome half of a [`Tree::open_node`] call: the background and the store
/// its payloads land in.
#[derive(Debug)]
pub(crate) struct ChromeInput<'a> {
    pub(crate) bg: &'a Background,
    /// Focus ring over the chrome; `Stroke::NONE` unless keyboard-focused.
    pub(crate) ring: Stroke,
    pub(crate) store: &'a mut RecordStore,
}

impl Tree {
    /// Exclusive pre-order end for node `i`, grid flag stripped. This and
    /// [`Self::has_children`], [`Self::parent_of`] and [`Self::subtree_has_grid`] take
    /// flat indices, not [`NodeId`]s, since callers walk the pre-order array.
    #[inline]
    pub(crate) fn subtree_end_of(&self, i: usize) -> usize {
        self.records.subtree_end()[i].end() as usize
    }

    #[inline]
    pub(crate) fn has_children(&self, i: usize) -> bool {
        self.records.subtree_end()[i].has_children(i)
    }

    #[inline]
    pub(crate) fn parent_of(&self, i: usize) -> Option<NodeId> {
        let parent = self.records.parent()[i];
        (parent != NodeId::NONE).then_some(parent)
    }

    #[inline]
    pub(crate) fn subtree_has_grid(&self, i: usize) -> bool {
        self.records.subtree_end()[i].has_grid()
    }

    pub(crate) fn pre_record(&mut self) {
        let mut last = mem::take(&mut self.last_inputs);
        last.take_from(self);
        self.last_inputs = last;
        self.focus_ring = ShapeStroke::default();
        self.shapes.records.clear();
        self.paint_anims.clear();
        self.roots.clear();
    }

    pub(crate) fn post_record(&mut self) {
        debug_assert!(
            self.paint_anims
                .entries
                .last()
                .is_none_or(|entry| entry.shape_idx < self.shapes.records.len() as u32),
            "paint animation shape index exceeds shapes.records",
        );
        let n = self.records.len();
        self.fingerprint.paint_counts =
            paint_counts(self.shapes.records.len(), self.chrome_table.len(), n);
        // Unchanged inputs keep their rollups: a compare instead of a hash per node.
        if self.last_inputs.hold_for(self) {
            #[cfg(debug_assertions)]
            self.assert_rollups_hold();
        } else {
            self.roll_up();
        }
        self.last_inputs.note_rolled();
    }

    fn roll_up(&mut self) {
        let n = self.records.len();
        self.rollups.reset_for(n);
        self.container_text.clear();
        self.container_text.grow(n);
        self.compute_rollups();
    }

    #[cfg(debug_assertions)]
    fn assert_rollups_hold(&mut self) {
        let mut check = mem::take(&mut self.rollup_check);
        mem::swap(&mut check.rollups, &mut self.rollups);
        mem::swap(&mut check.container_text, &mut self.container_text);
        let cascade_static = self.fingerprint.cascade_static;
        self.roll_up();
        assert_eq!(self.rollups.node, check.rollups.node, "kept node hashes");
        assert_eq!(
            self.rollups.subtree, check.rollups.subtree,
            "kept subtree hashes"
        );
        assert_eq!(
            self.rollups.layout_subtree, check.rollups.layout_subtree,
            "kept layout subtree hashes",
        );
        // By bits set: `clear` keeps a bit set's length, so equal owner sets can differ.
        assert!(
            self.container_text.ones().eq(check.container_text.ones()),
            "kept container text",
        );
        assert_eq!(
            self.fingerprint.cascade_static, cascade_static,
            "kept cascade static"
        );
        self.rollup_check = check;
    }

    /// Fused reverse-pre-order pass: computes the hash columns and discovers
    /// non-leaf direct-text owners.
    ///
    /// The full hash is derived from the layout half (what measure and arrange
    /// read) plus chrome and shape paint hashes, so the measure cache (keyed on the
    /// layout half) and cascade/damage (keyed on the full one) cannot drift. If
    /// layout might read an input it goes in the layout half: an extra input costs
    /// a cache miss, a missing one is a stale layout.
    fn compute_rollups(&mut self) {
        let n = self.records.len();
        let layouts = self.records.layout();
        let attrs = self.records.attrs();
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
        let SubtreeRollups {
            node,
            subtree,
            layout_subtree,
        } = &mut self.rollups;
        let container_text = &mut self.container_text;
        let cascade_static = &mut self.fingerprint.cascade_static;
        let node_out = node.as_mut_slice();
        let subtree_out = subtree.as_mut_slice();
        let layout_subtree_out = layout_subtree.as_mut_slice();
        let mut cascade_static_hasher = Hasher::new();

        for i in (0..n).rev() {
            let mut lh = Hasher::new();
            let mut ph = Hasher::new();
            layouts[i].hash_with_flags(attrs[i], &mut lh);
            let ex = extras[i];
            let mut tab_index = 0;
            if let Some(s) = ex.bounds {
                let bounds = &bounds_tab[s.idx()];
                bounds.hash(&mut lh);
                tab_index = bounds.tab_index;
            }
            // The transform moves no rect, so it rides the paint half: the measure cache
            // keeps hitting under a pan. It still dirties `node_hash` (pinned by
            // `self_transform_change_flips_node_hash`).
            let panel = ex.panel.map(|s| &panel_tab[s.idx()]);
            if let Some(panel) = panel {
                panel.hash_layout(&mut lh);
            }
            let transformed = panel.filter(|p| !p.transform.is_identity());
            // What the cascade's structural tables are built from: identity, nesting,
            // flag word, visibility and Tab key. Nesting makes this describe the tree's
            // shape. The top bit of the end word says whether a Tab key follows.
            const {
                assert!(
                    32 + NodeFlags::WIDTH + u8::BITS <= 63,
                    "the flag word and visibility overrun the Tab bit",
                );
            }
            cascade_static_hasher.write_u64(widget_ids[i].0);
            cascade_static_hasher.write_u64(
                u64::from(subtree_ends[i].end())
                    | (u64::from(attrs[i].bits()) << 32)
                    | (u64::from(layouts[i].meta.visibility() as u8) << (32 + NodeFlags::WIDTH))
                    | (u64::from(tab_index != 0) << 63),
            );
            if tab_index != 0 {
                cascade_static_hasher.write_u16(tab_index.cast_unsigned());
            }
            // A leading byte says which optional payloads follow (chrome, transform), so
            // no stream reads as another's.
            let chrome = ex.chrome.map(|s| chrome_tab[s.idx()].hash);
            ph.write_u8(u8::from(chrome.is_some()) | (u8::from(transformed.is_some()) << 1));
            if let Some(hash) = chrome {
                ph.write_u64(hash.0);
            }
            if let Some(panel) = transformed {
                panel.hash_transform(&mut ph);
            }

            // Walk direct shapes and child markers in record order via `TreeItems`, as the
            // encoder does. Child markers carry the child's `WidgetId` behind a `0xFF`
            // separator, so a reorder flips the hash and the damage diff emits the
            // order-inversion damage. Re-keying a child also flips the parent chain: a
            // one-frame cache miss, accepted as rare.
            //
            // Children's finalized subtree rollups fold in as they are yielded (children
            // are visited earlier in reverse pre-order); node hashes are appended after
            // `finish`.
            let mut sh = Hasher::new();
            let mut lsh = Hasher::new();
            let mut has_children = false;
            let mut has_direct_text = false;
            for item in TreeItems::new(&self.records, &self.shapes.records, NodeId(i as u32)) {
                match item {
                    TreeItem::ShapeRecord(idx, shape) => {
                        ph.write_u64(shape_hashes[idx as usize].0);
                        if shape.hash_layout_inputs(&mut lh) {
                            has_direct_text = true;
                        }
                    }
                    TreeItem::Child(c) => {
                        for hasher in [&mut lh, &mut ph] {
                            hasher.write_u8(0xFF);
                            hasher.write_u64(widget_ids[c.id.idx()].0);
                        }
                        sh.write_u64(subtree_out[c.id.idx()].0);
                        lsh.write_u64(layout_subtree_out[c.id.idx()].0);
                        has_children = true;
                    }
                }
            }
            let meta = layouts[i].meta;
            let mode = LayoutMode::from(meta);
            if has_direct_text && mode != LayoutMode::Leaf {
                container_text.insert(i);
            }
            // A container run is paint-only, so a node that paints nothing, and every
            // node under it, has no run to shape. A hidden leaf still shapes because the
            // run gives its slot extent (`LayoutPass::measure` tests `is_collapsed`).
            // Descendants' bits are already set in reverse pre-order.
            if !meta.visibility().is_visible() {
                container_text.remove_range(i..subtree_ends[i].end() as usize);
            }
            match mode {
                LayoutMode::Grid(id) => {
                    grid_defs[usize::from(id)].hash_visual(grid_tracks, &mut lh);
                }
                LayoutMode::Scrollbars(id) => {
                    scrollbar_defs[usize::from(id)].def.hash_visual(&mut lh);
                }
                _ => {}
            }
            let layout_hash = lh.finish();
            ph.write_u64(layout_hash);
            let node_hash = ph.finish();
            node_out[i] = ContentHash(node_hash);

            // A childless subtree is the node alone: its hash is the rollup.
            if has_children {
                sh.write_u64(node_hash);
                lsh.write_u64(layout_hash);
                subtree_out[i] = ContentHash(sh.finish());
                layout_subtree_out[i] = ContentHash(lsh.finish());
            } else {
                subtree_out[i] = ContentHash(node_hash);
                layout_subtree_out[i] = ContentHash(layout_hash);
            }
        }
        *cascade_static = ContentHash(cascade_static_hasher.finish());
    }

    pub(crate) fn push_scrollbars_def(&mut self, def: ResolvedScrollbarsDef) -> ScrollbarsDefId {
        let id = ScrollbarsDefId::from_index(self.scrollbar_defs.len());
        self.scrollbar_defs.push(def);
        id
    }

    pub(crate) fn push_grid_def(&mut self, rows: &[Track], cols: &[Track]) -> GridDefId {
        let id = GridDefId::from_index(self.grid_defs.len());
        self.grid_tracks.reserve(rows.len() + cols.len());
        let row_start = self.grid_tracks.len();
        self.grid_tracks.extend_from_slice(rows);
        let col_start = self.grid_tracks.len();
        self.grid_tracks.extend_from_slice(cols);
        self.grid_defs.push(GridDef {
            rows: Span::from(row_start..col_start),
            cols: Span::from(col_start..self.grid_tracks.len()),
        });
        id
    }

    /// Push a node under the open node (or as a new root if none is open) and make
    /// it the tip. Roots stamp `scratch.pending_placement` onto the new `RootSlot`.
    ///
    /// `chrome` is `None` for nodes without a background; `ClipMode::Rounded` then
    /// downgrades to `Rect`. With chrome, `Rounded` keeps the row even if
    /// `Background::is_noop`, so the encoder can read `bg.radius`.
    #[inline]
    pub(super) fn open_node(
        &mut self,
        scratch: &mut RecordingScratch,
        widget_id: WidgetId,
        node: &Node,
        chrome: Option<ChromeInput<'_>>,
    ) -> NodeId {
        // Overflow is guarded in `SubtreeEnd::new_open` (31-bit arena ceiling).
        let new_id = NodeId(self.records.len() as u32);

        let parent_frame = scratch.open_frames.last().copied();

        if parent_frame.is_none() {
            let pending = scratch.pending_placement.unwrap_or_default();
            self.roots.push(RootSlot {
                first_node: new_id,
                placement: pending,
            });
        }
        let mut cols = node.columns(widget_id);
        // A root raised from a disabled scope is disabled, so the cascade disables the
        // whole overlay.
        if parent_frame.is_none() && scratch.owner_disabled {
            cols.attrs.set_disabled(true);
        }
        // A rounded clip with no radius is a plain scissor. Applied to the recorded
        // flags: only here are the request and the chrome both visible.
        if cols.attrs.clip_mode() == ClipMode::Rounded
            && chrome
                .as_ref()
                .is_none_or(|c| c.bg.corners.is_approx_zero())
        {
            cols.attrs.set_clip(ClipMode::Rect);
        }
        // Decoded once; `meta` is immutable past `columns` except `padding`.
        let mode = LayoutMode::from(cols.layout.meta);
        match mode {
            LayoutMode::Grid(id) => debug_assert!(
                usize::from(id) < self.grid_defs.len(),
                "LayoutMode::Grid id {id:?} references no grid_def — only Grid::show should push grid nodes",
            ),
            LayoutMode::Scrollbars(id) => debug_assert!(
                usize::from(id) < self.scrollbar_defs.len(),
                "LayoutMode::Scrollbars id {id:?} references no scrollbar_def — only Widget::scrollbar_def installs one",
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
        if let Some(ChromeInput {
            bg,
            ring: focus_ring,
            store,
        }) = chrome
        {
            // A border paints inside the arranged rect, so `padding` grows by it. Done
            // here so layout columns carry the effective padding and the `LayoutCore`
            // hash invalidates `MeasureCache` when it shifts.
            let ring = bg.border_inset();
            if ring != 0.0 {
                let [l, t, r, b] = cols.layout.padding.as_array();
                cols.layout.padding = Spacing::new(l + ring, t + ring, r + ring, b + ring);
            }
            // Storage noop gate for chrome; mirrors `Shapes::add`.
            let needs_chrome_row = !bg.is_noop()
                || !focus_ring.is_noop()
                || matches!(cols.attrs.clip_mode(), ClipMode::Rounded);
            if needs_chrome_row {
                let row = lower::background(store, bg, focus_ring);
                if row.ring {
                    self.focus_ring = ShapeStroke::from(focus_ring);
                }
                ex.chrome = Some(Index16::new(self.chrome_table.len(), "chrome_table"));
                self.chrome_table.push(row);
            }
        }
        // Stamp the self-Grid bit now so `close_node` skips a `layout[i].meta` read.
        let init_end = SubtreeEnd::new_open(new_id.0, matches!(mode, LayoutMode::Grid(_)));
        self.records.push(NodeRecord {
            widget_id: cols.widget_id,
            shape_span: Span::new(self.shapes.records.len() as u32, 0),
            subtree_end: init_end,
            parent: parent_frame.map_or(NodeId::NONE, |f| f.node),
            layout: cols.layout,
            attrs: cols.attrs,
            extras: ex,
        });
        let ancestor_or_self_disabled =
            parent_frame.is_some_and(|f| f.ancestor_or_self_disabled) || cols.attrs.is_disabled();
        let effectively_visible = parent_frame.is_none_or(|f| f.effectively_visible)
            && cols.layout.meta.visibility().is_visible();
        // The child adds one marker row to the parent's paint span; its own counter
        // starts past its chrome row.
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

    /// Range-check a child's `grid` cell against its parent's `GridDef`. Gated
    /// whole: everything it does exists to reach the assert, and this runs on every
    /// node open.
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

    pub(super) fn close_node(&mut self, scratch: &mut RecordingScratch) {
        let popped = scratch
            .open_frames
            .pop()
            .expect("close_node called with no open node");
        let closing = popped.node;

        let i = closing.idx();
        let shapes_len = self.shapes.records.len() as u32;
        let shapes = &mut self.records.shape_span_mut()[i];
        shapes.len = shapes_len - shapes.start;

        // The paint-row stream is counted at record time (`OpenFrame::paint_rows`) and
        // re-derived at cascade time by `compute_paint_rect` walking `TreeItems`.
        // Nothing ties the two, so compare them here, debug-only, for every node (not
        // only those with a paint anim).
        debug_assert_eq!(
            popped.paint_rows,
            u32::from(self.chrome(closing).is_some())
                + TreeItems::new(&self.records, &self.shapes.records, closing).count() as u32,
            "paint-row count drifted from the cascade's row stream at node {i}",
        );

        // `subtree_end[i]` already answers "subtree contains Grid": self-Grid was
        // stamped at open and descendants merged up at close.
        let child_end = self.records.subtree_end()[i];

        if let Some(parent) = scratch.open_frames.last().map(|f| f.node) {
            let pi = parent.idx();
            self.records.subtree_end_mut()[pi].merge_child(child_end);
        }
    }

    /// Children of `parent` in declaration order, with collapse state.
    pub(crate) fn children(&self, parent: NodeId) -> ChildIter<'_> {
        ChildIter::new(&self.records, parent)
    }

    pub(crate) fn active_children(&self, parent: NodeId) -> impl Iterator<Item = NodeId> + '_ {
        self.children(parent).filter_map(Child::active)
    }

    /// This node's direct shapes interleaved with its immediate children, in record
    /// order. `compute_rollups` and `close_node` use [`TreeItems::new`] instead,
    /// as they hold a `&mut` on a `Tree` field while walking.
    pub(crate) fn tree_items(&self, node: NodeId) -> TreeItems<'_> {
        TreeItems::new(&self.records, &self.shapes.records, node)
    }

    /// This node's raw transform: `None` for non-panel nodes and identity
    /// transforms. Private; use [`Self::anchored_transform`].
    #[inline]
    fn transform_of(&self, id: NodeId) -> Option<TranslateScale> {
        self.records.extras()[id.idx()]
            .panel
            .map(|s| self.panel_table[s.idx()].transform)
            .filter(|t| !t.is_identity())
    }

    /// This node's transform, anchored so its scale pivots about the panel's own
    /// origin instead of the layer's `(0, 0)`. `rect` is the arranged rect. Cascade
    /// and encoder must both use this, or a scaled panel's body drifts from its
    /// damage rect.
    #[inline]
    pub(crate) fn anchored_transform(&self, id: NodeId, rect: Rect) -> Option<TranslateScale> {
        self.transform_of(id).map(|t| t.anchored_at(rect.min))
    }

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

    pub(crate) fn chrome(&self, id: NodeId) -> Option<&ChromeRow> {
        self.records.extras()[id.idx()]
            .chrome
            .map(|s| &self.chrome_table[s.idx()])
    }
}

fn paint_counts(shapes: usize, chrome_rows: usize, nodes: usize) -> ContentHash {
    let mut h = Hasher::new();
    h.write_usize(shapes);
    h.write_usize(chrome_rows);
    h.write_usize(nodes);
    ContentHash(h.finish())
}

#[cfg(test)]
pub(crate) mod internals {
    use crate::scene::tree::*;
    use crate::shape::record::ShapeRecord;

    impl Tree {
        /// Direct shapes of `node`, including parent-pushed sub-rects interleaved between children.
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
