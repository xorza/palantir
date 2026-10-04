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

/// A single layer's arena. Per-layer trees live on
/// [`crate::scene::forest::Forest`] and share no record/shape storage,
/// so a mid-recording `Ui::layer` call dispatches straight into the
/// destination tree without interleaving — no post-record reorder pass
/// is needed to separate the layers again.
///
/// **`records`** is `Soa<NodeRecord>` indexed by `NodeId.0`, in pre-order
/// paint order (parent before children, siblings in declaration order).
/// Reverse iteration gives topmost-first (used by hit-testing). `soa-rs`
/// lays each [`NodeRecord`] field out as its own contiguous slice, so
/// each pass touches only the bytes it needs. Which pass reads which
/// column is documented on the field itself.
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
    /// The focus ring's stroke, for the one chrome row that flags it —
    /// see [`ChromeRow::ring`]. A no-op when no node in this tree holds
    /// focus that came from the keyboard.
    pub(crate) focus_ring: ShapeStroke,

    /// Flat per-frame shape buffer. Records are indexed via
    /// `NodeRecord.shape_span`; variable-length payloads (mesh
    /// verts/indices, polyline points/colors, gradients) live on the
    /// `RecordStore`.
    pub(crate) shapes: Shapes,

    pub(crate) grid_tracks: Vec<Track>,
    pub(crate) grid_defs: Vec<GridDef>,
    /// Side table for [`LayoutMode::Scrollbars`], same arrangement as
    /// `grid_defs`: the def is too wide for the packed 16-bit payload.
    pub(crate) scrollbar_defs: Vec<ResolvedScrollbarsDef>,

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
    /// Non-leaf owners of direct text shapes, restricted to the ones
    /// that paint. Layout iterates the set after arrange to shape
    /// paint-only text against its final padded width — a worklist, not
    /// a hash, which is why it sits here rather than with the rollup
    /// columns.
    ///
    /// The visibility cascade is folded in at build time, so a consumer
    /// reads the set and shapes, with no ancestor walk and no test of
    /// its own.
    pub(crate) container_text: FixedBitSet,
}

/// The chrome half of a [`Tree::open_node`] call: the background to
/// lower, and the store its gradient and text payloads land in. One
/// parameter rather than two because a node either has chrome and both,
/// or has neither.
#[derive(Debug)]
pub(crate) struct ChromeInput<'a> {
    pub(crate) bg: &'a Background,
    /// The focus ring over the chrome — `Stroke::NONE` unless the node
    /// holds focus that came from the keyboard.
    pub(crate) ring: Stroke,
    pub(crate) store: &'a mut RecordStore,
}

impl Tree {
    /// Exclusive pre-order end for node `i`, grid flag stripped.
    ///
    /// `usize` rather than the stored `u32`: every caller indexes or spans
    /// with the result, so the cast belongs here once instead of at each
    /// of them. The one that stores it again narrows explicitly.
    ///
    /// **A flat-index accessor, not a [`NodeId`] one** — as are
    /// [`Self::has_children`], [`Self::parent_of`] and
    /// [`Self::subtree_has_grid`]. Their callers walk the pre-order
    /// array by interval (`[start, end)`) rather than looking one node
    /// up, so a `NodeId` parameter would only make each of them convert
    /// back. The `NodeId` family is [`Self::children`], [`Self::bounds`],
    /// [`Self::panel`], [`Self::chrome`] and [`Self::transform_of`].
    #[inline]
    pub(crate) fn subtree_end_of(&self, i: usize) -> usize {
        self.records.subtree_end()[i].end() as usize
    }

    /// Whether node `i` has any children — see
    /// [`SubtreeEnd::has_children`].
    #[inline]
    pub(crate) fn has_children(&self, i: usize) -> bool {
        self.records.subtree_end()[i].has_children(i)
    }

    /// The immediate parent of node `i`, or `None` when `i` is a root.
    #[inline]
    pub(crate) fn parent_of(&self, i: usize) -> Option<NodeId> {
        let parent = self.records.parent()[i];
        (parent != NodeId::NONE).then_some(parent)
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
        self.focus_ring = ShapeStroke::default();
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
        debug_assert!(
            self.paint_anims
                .entries
                .last()
                .is_none_or(|entry| entry.shape_idx < self.shapes.records.len() as u32),
            "paint animation shape index exceeds shapes.records",
        );
        let n = self.records.len();
        self.rollups.reset_for(n);
        // `clear` keeps the length, so the grow only does work the first
        // time the tree reaches this size. Sized here rather than at the
        // insert site so `compute_rollups`' loop carries no sizing call.
        self.container_text.clear();
        self.container_text.grow(n);
        self.fingerprint.paint_counts =
            paint_counts(self.shapes.records.len(), self.chrome_table.len(), n);
        self.compute_rollups();
    }

    /// Fused reverse-pre-order pass: computes the hash columns and
    /// discovers non-leaf direct-text owners in a single sweep.
    /// `subtree[i]` reads `node[i]` (just written this iteration) and
    /// the already-finalized `subtree[children]` (visited earlier in
    /// the reverse pass).
    ///
    /// **Layout half first, full hash derived from it.** A node's layout
    /// hash folds what measure and arrange read — the layout core and
    /// flags, bounds, panel, grid and scrollbar definitions, child ids in
    /// order, and each text run's shaping inputs. Its full hash is that,
    /// plus chrome and every shape's paint hash in record order. An
    /// input added to the layout half reaches the full half by
    /// construction, so the measure cache (which keys on the layout
    /// half) and the cascade and damage (which key on the full one)
    /// cannot drift apart. The rule for a new input: if layout might read
    /// it, it goes in the layout half — an extra input costs a cache miss,
    /// a missing one is a stale layout.
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
        let SubtreeRollups {
            node,
            subtree,
            layout_subtree,
        } = &mut self.rollups;
        let container_text = &mut self.container_text;
        // `paint_counts` is stamped by `post_record` before this pass —
        // a whole-tree fold, not a per-node one.
        let cascade_static = &mut self.fingerprint.cascade_static;
        let node_out = node.as_mut_slice();
        let subtree_out = subtree.as_mut_slice();
        let layout_subtree_out = layout_subtree.as_mut_slice();
        let mut cascade_static_hasher = Hasher::new();

        for i in (0..n).rev() {
            // `lh` is the layout half, `ph` what only paint reads.
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
            // The transform moves no rect, so it rides the paint half:
            // the measure cache keeps hitting under a pan, and the
            // cascade refreshes geometry in place rather than rebuilding.
            // It still dirties `node_hash`, which it must — direct shapes
            // paint inside it per the `Panel::transform` contract. Pinned
            // by `self_transform_change_flips_node_hash`.
            let panel = ex.panel.map(|s| &panel_tab[s.idx()]);
            if let Some(panel) = panel {
                panel.hash_layout(&mut lh);
            }
            let transformed = panel.filter(|p| !p.transform.is_identity());
            // What the cascade's structural tables are built from, and
            // nothing else: identity, nesting, the flag word, visibility
            // and the Tab order key. Nesting is what makes this hash
            // describe the tree's *shape* — without it two trees with
            // the same nodes nested differently collide. The flag word
            // and visibility share the end's word, and its top bit says
            // whether a Tab key follows.
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
            // One leading byte says which of the two optional payloads
            // follow — chrome, whose authoring hash was computed at
            // lowering time (`shapes::lower::background`), and the
            // transform — so no stream can be read as another's.
            let chrome = ex.chrome.map(|s| chrome_tab[s.idx()].hash);
            ph.write_u8(u8::from(chrome.is_some()) | (u8::from(transformed.is_some()) << 1));
            if let Some(hash) = chrome {
                ph.write_u64(hash.0);
            }
            if let Some(panel) = transformed {
                panel.hash_transform(&mut ph);
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
            // The subtree hashers ride the same walk: each child's
            // already-finalized subtree rollups (reverse pre-order —
            // children were visited earlier) fold in as it's yielded, and
            // the node hashes are appended after `finish` below —
            // children-then-self, one traversal instead of a second
            // child-hop loop.
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
            // One load and one decode for the four consumers below.
            let meta = layouts[i].meta;
            let mode = LayoutMode::from(meta);
            if has_direct_text && mode != LayoutMode::Leaf {
                container_text.insert(i);
            }
            // A container run is paint-only, so a node that paints
            // nothing — and every node under it — has no run to shape.
            // A hidden *leaf* is the opposite case and still shapes,
            // because there the run is what gives the slot its extent,
            // which is why `LayoutPass::measure` tests `is_collapsed`
            // where this tests `!is_visible`. Reverse pre-order is what
            // makes one range clear enough: the descendants' bits are
            // already in place when their ancestor reaches this line.
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

            // Childless subtree = the node alone, so the node hash IS the
            // rollup — skip the second hasher round-trip (most nodes).
            // Inner nodes fold children (streamed above) then self.
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

    /// Intern one bar overlay's def, returning the id its node packs.
    /// Called only by `Widget::scrollbar_def`, which is why the
    /// `open_node` debug assert can treat a dangling id as a caller bug.
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
        node: &Node,
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
        let mut cols = node.columns(widget_id);
        // A root raised from a disabled scope is disabled, as a child of
        // that scope's node would be — set on the record so the cascade
        // disables the whole overlay, not only what records against it.
        if parent_frame.is_none() && scratch.owner_disabled {
            cols.attrs.set_disabled(true);
        }
        // A rounded clip with no radius to round is a plain scissor.
        // Applied to the recorded flags rather than to the node, because
        // this is the only hop that sees both the node's request and the
        // chrome supplying the radius.
        if cols.attrs.clip_mode() == ClipMode::Rounded
            && chrome
                .as_ref()
                .is_none_or(|c| c.bg.corners.is_approx_zero())
        {
            cols.attrs.set_clip(ClipMode::Rect);
        }
        // Decoded once — the def-handle asserts and the self-Grid stamp
        // below all want it, and `meta` is immutable past `columns`
        // (only `padding` is rewritten, by the stroke inflation).
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
            // A chrome border paints fully inside the node's arranged
            // rect (see `quad_pipeline/shader.wgsl` SDF stroke band), so `padding` grows
            // by the border on every side and children sit inside it
            // without the user having to add it by hand.
            // Done here (not in the layout pass) so the layout columns
            // already carry the effective padding — zero hot-path cost
            // and the LayoutCore hash invalidates `MeasureCache`
            // automatically when the inflated value shifts.
            let ring = bg.border_inset();
            if ring != 0.0 {
                let [l, t, r, b] = cols.layout.padding.as_array();
                cols.layout.padding = Spacing::new(l + ring, t + ring, r + ring, b + ring);
            }
            // Tree-storage noop gate for chrome — mirrors `Shapes::add`
            // for the shape buffer and `PaintSink::draw_*` for emits.
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
        // Stamp the self-Grid bit at open time — the mode is already
        // decoded above. Lets `close_node` drop its `layout[i].meta` read
        // (3 record columns → 2). `new_open` asserts the 31-bit arena
        // ceiling (high bit is the grid flag).
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
        ChildIter::new(&self.records, parent)
    }

    /// Iterate non-collapsed children of `parent`, yielding `NodeId`s
    /// directly. Equivalent to `children(parent).filter_map(Child::active)`
    /// but shorter at call sites — most layout drivers want this form.
    pub(crate) fn active_children(&self, parent: NodeId) -> impl Iterator<Item = NodeId> + '_ {
        self.children(parent).filter_map(Child::active)
    }

    /// This node's direct shapes interleaved with its immediate
    /// children, in record order.
    ///
    /// Two walks inside this type reach for [`TreeItems::new`] instead:
    /// `compute_rollups` and `close_node` both hold a `&mut` on one of
    /// `Tree`'s fields while they walk, and `&self` here would take the
    /// whole of it. Every caller that *can* take `&self` comes through
    /// this.
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
