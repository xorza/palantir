//! Per-plane arena tree: SoA `records` column, sparse side tables, flat shape
//! buffer, and subtree-rollup hashes for cross-frame caches.
//!
//! Noop filtering at this storage tier: `Shapes::add` drops `Shape::is_noop`
//! shapes and `Tree::open_node` drops `Chrome::is_noop` chrome rows (policy in
//! [`paint_sink`](crate::renderer::frontend::paint_sink)). Partial-noop chrome is
//! dropped per emit downstream.

pub(crate) mod extras_idx;
pub(crate) mod iter;
pub(crate) mod node_id;
pub(crate) mod node_record;
pub(crate) mod paint_animations;
pub(crate) mod pane_entry;
pub(crate) mod recording_scratch;
mod rollup_inputs;
pub(crate) mod root_slot;
pub(crate) mod subtree_end;
pub(crate) mod subtree_rollups;
pub(crate) mod tree_fingerprint;

use crate::common::content_hash::ContentHash;
use crate::common::hash::Hasher;
use crate::common::non_max::NonMaxU32;
use crate::common::span::Span;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::geometry::translate_scale::TranslateScale;
use crate::primitives::layout::clip_mode::ClipMode;
use crate::primitives::layout::grid_def::GridDef;
use crate::primitives::layout::layout_mode::{GridDefId, LayoutMode, ScrollbarsDefId};
use crate::primitives::layout::track::Track;
use crate::primitives::layout::visibility::Visibility;
use crate::primitives::paint::border::Border;
use crate::primitives::paint::chrome::Chrome;
use crate::scene::node::bounds_extras::BoundsExtras;
use crate::scene::node::inherited::Inherited;
use crate::scene::node::node_cols::NodeCols;
use crate::scene::node::node_flags::NodeFlags;
use crate::scene::node::panel_extras::PanelExtras;
use crate::scene::scrollbars_def::ResolvedScrollbarsDef;
use crate::scene::tree::extras_idx::ExtrasIdx;
use crate::scene::tree::iter::{Child, ChildIter, TreeItem, TreeItems};
use crate::scene::tree::node_id::NodeId;
use crate::scene::tree::node_record::NodeRecord;
use crate::scene::tree::paint_animations::PaintAnimations;
use crate::scene::tree::pane_entry::{PaneEntry, PaneOrigin};
use crate::scene::tree::recording_scratch::{OpenFrame, RecordingScratch};
#[cfg(debug_assertions)]
use crate::scene::tree::rollup_inputs::RollupCheck;
use crate::scene::tree::rollup_inputs::{RecordedColumns, RollupInputs};
use crate::scene::tree::root_slot::RootSlot;
use crate::scene::tree::subtree_end::SubtreeEnd;
use crate::scene::tree::subtree_rollups::SubtreeRollups;
use crate::scene::tree::tree_fingerprint::TreeFingerprint;
use crate::shape::lower;
use crate::shape::paint::chrome_row::ChromeRow;
use crate::shape::paint::lowered_border::LoweredBorder;
use crate::shape::paint::lowered_material::{LoweredMaterial, MaterialRead};
use crate::shape::record_store::RecordStore;
use crate::shape::shapes::Shapes;
use fixedbitset::FixedBitSet;
use soa_rs::Soa;
use std::hash::{Hash, Hasher as _};

/// The lane the cascade's static hash gives a node's [`Visibility`], above its flag word.
const VISIBILITY_BITS: u32 = 2;

/// A single plane's arena. Planes share no storage, so `Ui::layer` records
/// straight into its destination tree. `records` is in pre-order paint order;
/// reverse iteration is topmost-first.
#[derive(Debug, Default)]
pub(crate) struct Tree {
    pub(crate) records: Soa<NodeRecord>,

    pub(crate) bounds_table: Vec<BoundsExtras>,
    pub(crate) panel_table: Vec<PanelExtras>,
    /// One row per node with chrome or `ClipMode::Rounded`; a rounded clip keeps its
    /// row even for a noop paint so the encoder can read `chrome.radius`.
    pub(crate) chrome_table: Vec<ChromeRow>,
    /// The focus ring's stroke for the one chrome row that flags it ([`ChromeRow::ring`]).
    pub(crate) focus_ring: LoweredBorder,

    pub(crate) shapes: Shapes,

    pub(crate) grid_tracks: Vec<Track>,
    pub(crate) grid_defs: Vec<GridDef>,
    /// Side table for [`LayoutMode::Scrollbars`]; the def is too wide to pack.
    pub(crate) scrollbar_defs: Vec<ResolvedScrollbarsDef>,

    pub(crate) roots: Vec<RootSlot>,

    /// Sparse shape-keyed paint animations, cleared in `pre_record`. Sampling is a
    /// pure function of `now`, so no timestamps are stored.
    pub(crate) paint_animations: PaintAnimations,
    /// The backdrop panes recorded this frame, in shape order; cleared in `pre_record`.
    pub(crate) panes: Vec<PaneEntry>,

    pub(crate) rollups: SubtreeRollups,
    pub(crate) fingerprint: TreeFingerprint,
    /// Non-leaf owners of direct text shapes that paint. Layout shapes their text
    /// after arrange. Visibility is folded in at build time.
    pub(crate) container_text: FixedBitSet,
    last_inputs: RollupInputs,
    #[cfg(debug_assertions)]
    rollup_check: RollupCheck,
}

/// What paints behind a node [`Forest::open_node`](crate::scene::forest::Forest::open_node) opens: its chrome, borrowed so a chromed widget doesn't re-copy it down the record chain, and the focus ring over it.
#[derive(Clone, Copy, Debug)]
pub(crate) struct NodeChrome<'a> {
    /// `None` for a chrome-less node.
    pub(crate) chrome: Option<&'a Chrome>,
    /// `Border::NONE` unless the node shows keyboard focus.
    pub(crate) ring: Border,
    /// What the chrome's material reads this record; `None` where it paints none.
    pub(crate) material: Option<MaterialRead>,
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

    /// Ready the tree for a record pass. Destructured, so every field states what a pass does
    /// with it: the columns the rollups read are swapped out as the last pass's, the per-pass
    /// rows cleared, and the rollups kept for `post_record`, which holds them or computes them
    /// again.
    pub(crate) fn pre_record(&mut self) {
        let Self {
            records,
            bounds_table,
            panel_table,
            chrome_table,
            focus_ring,
            shapes:
                Shapes {
                    records: shape_records,
                    hashes: shape_hashes,
                },
            grid_tracks,
            grid_defs,
            scrollbar_defs,
            roots,
            paint_animations,
            panes,
            rollups: _,
            fingerprint: _,
            container_text: _,
            last_inputs,
            #[cfg(debug_assertions)]
                rollup_check: _,
        } = self;
        last_inputs.take_from(RecordedColumns {
            records,
            bounds_table,
            panel_table,
            chrome_table,
            shape_hashes,
            grid_tracks,
            grid_defs,
            scrollbar_defs,
        });
        *focus_ring = LoweredBorder::default();
        shape_records.clear();
        paint_animations.clear();
        panes.clear();
        roots.clear();
    }

    pub(crate) fn post_record(&mut self) {
        debug_assert!(
            self.paint_animations
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
        let mut check = std::mem::take(&mut self.rollup_check);
        std::mem::swap(&mut check.rollups, &mut self.rollups);
        std::mem::swap(&mut check.container_text, &mut self.container_text);
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
            let tab_idx = if let Some(s) = ex.bounds {
                let bounds = &bounds_tab[s.idx()];
                bounds.hash(&mut lh);
                bounds.tab_idx
            } else {
                0
            };
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
                    (Visibility::Collapsed as u32) < 1 << VISIBILITY_BITS,
                    "visibility outgrows its lane",
                );
                assert!(
                    32 + NodeFlags::WIDTH + VISIBILITY_BITS <= 63,
                    "the flag word and visibility overrun the Tab bit",
                );
            }
            widget_ids[i].hash(&mut cascade_static_hasher);
            cascade_static_hasher.write_u64(
                u64::from(subtree_ends[i].end())
                    | (u64::from(attrs[i].bits()) << 32)
                    | (u64::from(layouts[i].meta.visibility() as u8) << (32 + NodeFlags::WIDTH))
                    | (u64::from(tab_idx != 0) << 63),
            );
            if tab_idx != 0 {
                cascade_static_hasher.write_u16(tab_idx.cast_unsigned());
            }
            // A leading byte says which optional payloads follow (chrome, transform), so
            // no stream reads as another's.
            let chrome = ex.chrome.map(|s| chrome_tab[s.idx()].hash);
            ph.write_u8(u8::from(chrome.is_some()) | (u8::from(transformed.is_some()) << 1));
            if let Some(hash) = chrome {
                hash.hash(&mut ph);
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
                        shape_hashes[idx as usize].hash(&mut ph);
                        if shape.hash_layout_inputs(&mut lh) {
                            has_direct_text = true;
                        }
                    }
                    TreeItem::Child(c) => {
                        for hasher in [&mut lh, &mut ph] {
                            hasher.write_u8(0xFF);
                            widget_ids[c.id.idx()].hash(hasher);
                        }
                        subtree_out[c.id.idx()].hash(&mut sh);
                        layout_subtree_out[c.id.idx()].hash(&mut lsh);
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
            // Each mode's payload, which `LayoutCore::hash_with_flags` leaves out; exhaustive, so a
            // new mode states what it hashes. A side-table id is a per-frame index, so its
            // definition is hashed, not the id.
            match mode {
                LayoutMode::Leaf | LayoutMode::ZStack | LayoutMode::Canvas => {}
                LayoutMode::Stack(axis) | LayoutMode::WrapStack(axis) => axis.hash(&mut lh),
                LayoutMode::Scroll(axes) => axes.hash(&mut lh),
                LayoutMode::Grid(id) => {
                    grid_defs[usize::from(id)].hash_visual(grid_tracks, &mut lh);
                }
                LayoutMode::Scrollbars(id) => {
                    scrollbar_defs[usize::from(id)].def.hash_visual(&mut lh);
                }
            }
            let layout_hash = ContentHash::finish(&lh);
            layout_hash.hash(&mut ph);
            let node_hash = ContentHash::finish(&ph);
            node_out[i] = node_hash;

            // A childless subtree is the node alone: its hash is the rollup.
            if has_children {
                node_hash.hash(&mut sh);
                layout_hash.hash(&mut lsh);
                subtree_out[i] = ContentHash::finish(&sh);
                layout_subtree_out[i] = ContentHash::finish(&lsh);
            } else {
                subtree_out[i] = node_hash;
                layout_subtree_out[i] = layout_hash;
            }
        }
        *cascade_static = ContentHash::finish(&cascade_static_hasher);
    }

    pub(crate) fn push_scrollbars_def(&mut self, def: ResolvedScrollbarsDef) -> ScrollbarsDefId {
        let id = ScrollbarsDefId::from_idx(self.scrollbar_defs.len());
        self.scrollbar_defs.push(def);
        id
    }

    pub(crate) fn push_grid_def(&mut self, rows: &[Track], cols: &[Track]) -> GridDefId {
        let id = GridDefId::from_idx(self.grid_defs.len());
        self.grid_tracks.reserve(rows.len() + cols.len());
        let row_start = self.grid_tracks.len();
        self.grid_tracks.extend_from_slice(rows);
        let col_start = self.grid_tracks.len();
        self.grid_tracks.extend_from_slice(cols);
        self.grid_defs.push(GridDef {
            rows: Span::try_from(row_start..col_start).unwrap(),
            cols: Span::try_from(col_start..self.grid_tracks.len()).unwrap(),
        });
        id
    }

    /// Push a node's columns under the open node (or as a new root if none is open)
    /// and make it the tip. Roots stamp `scratch.pending_placement` onto the new
    /// `RootSlot`.
    ///
    /// A node with no chrome and no ring downgrades `ClipMode::Rounded` to `Rect`. With
    /// chrome, `Rounded` keeps the row even if `Chrome::is_noop`, so the encoder can read
    /// `chrome.radius`; the chrome's payloads land in `store`.
    #[inline]
    pub(super) fn open_node(
        &mut self,
        scratch: &mut RecordingScratch,
        mut cols: NodeCols,
        node_chrome: NodeChrome<'_>,
        store: &mut RecordStore,
    ) -> NodeId {
        let NodeChrome {
            chrome,
            ring,
            material,
        } = node_chrome;
        // A ring with no chrome rides an empty one.
        let chrome = chrome.or_else(|| (!ring.is_noop()).then_some(&Chrome::NONE));
        // Overflow is guarded in `SubtreeEnd::new_open` (31-bit arena ceiling).
        let new_id = NodeId(self.records.len() as u32);

        let parent_frame = scratch.open_frames.last().copied();

        if parent_frame.is_none() {
            let pending = scratch.pending_placement.unwrap_or_default();
            self.roots.push(RootSlot {
                first_node: new_id,
                placement: pending,
                layer: scratch.pending_layer,
            });
        }
        // A root raised from a disabled scope is disabled, so the cascade disables the
        // whole overlay.
        if parent_frame.is_none() && scratch.owner_disabled {
            cols.attrs.set_disabled(true);
        }
        // A rounded clip with no radius is a plain scissor. Applied to the recorded
        // flags: only here are the request and the chrome both visible.
        if cols.attrs.clip_mode() == ClipMode::Rounded
            && chrome.is_none_or(|chrome| chrome.corners.is_approx_zero())
        {
            cols.attrs.set_clip(ClipMode::Rect);
        }
        // Decoded once; `meta` is immutable past `cols` except `padding`.
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

        let mut ex = ExtrasIdx::default();
        if !cols.bounds.is_default() {
            ex.bounds = Some(NonMaxU32::index(self.bounds_table.len()));
            self.bounds_table.push(cols.bounds);
        }
        if !cols.panel.is_default() {
            ex.panel = Some(NonMaxU32::index(self.panel_table.len()));
            self.panel_table.push(cols.panel);
        }
        if let Some(chrome) = chrome {
            // A border paints inside the arranged rect, so `padding` grows by it. Done
            // here so layout columns carry the effective padding and the `LayoutCore`
            // hash invalidates `MeasureCache` when it shifts.
            cols.layout.padding = chrome.fold_padding(cols.layout.padding);
            // Storage noop gate for chrome; mirrors `Shapes::add`.
            let needs_chrome_row = !chrome.is_noop()
                || !ring.is_noop()
                || matches!(cols.attrs.clip_mode(), ClipMode::Rounded);
            if needs_chrome_row {
                let row = lower::chrome(store, chrome, ring, material);
                if row.ring {
                    self.focus_ring = LoweredBorder::lower(ring);
                }
                ex.chrome = Some(NonMaxU32::index(self.chrome_table.len()));
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
        let inherited = parent_frame
            .map_or(Inherited::ROOT, |f| f.inherited)
            .child(cols.attrs, cols.layout.meta.visibility());
        // A chrome whose material captures is a pane at the node's first row.
        if !inherited.invisible
            && ex.chrome.is_some_and(|row| {
                self.chrome_table[row.idx()]
                    .material
                    .is_some_and(LoweredMaterial::captures)
            })
        {
            self.panes.push(PaneEntry {
                node: new_id,
                shape_idx: self.shapes.records.len() as u32,
                origin: PaneOrigin::Chrome,
                row: 0,
            });
        }
        // The child adds one marker row to the parent's paint span; its own counter
        // starts past its chrome row.
        if let Some(parent) = scratch.open_frames.last_mut() {
            parent.paint_rows += 1;
        }
        scratch.open_frames.push(OpenFrame {
            node: new_id,
            inherited,
            paint_rows: u32::from(ex.chrome.is_some()),
        });
        new_id
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
    /// origin instead of the plane's `(0, 0)`. `rect` is the arranged rect. Cascade
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
    ContentHash::of((shapes, chrome_rows, nodes))
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
