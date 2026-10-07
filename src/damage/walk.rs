//! The per-layer structural diff: [`LayerWalk`] and the [`Tier`] it sorts each node into.
//!
//! [`NodeSnapshot`] is `Copy`, so [`LayerWalk::classify`] decides on a copy and returns a plain [`Tier`]; deciding on a live map `Entry` would cost each arm its `&mut self`.

use crate::cascade::layer_cascade::LayerCascade;
use crate::cascade::paint::{Paint, PaintRows};
use crate::common::block_arena::BlockArena;
use crate::common::span::Span;
use crate::damage;
use crate::damage::counters::DamageCounters;
use crate::damage::inverted_overlaps::InvertedOverlaps;
use crate::damage::node_snapshot::NodeSnapshot;
use crate::damage::row_matcher::RowMatcher;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::identity::widget_id::{WidgetId, WidgetIdMap};
use crate::scene::layer::Layer;
use crate::scene::tree::Tree;
use crate::scene::tree::iter::TreeItem;
use crate::scene::tree::node_id::NodeId;

/// What the diff decided about one node, cheapest and most common first.
#[derive(Clone, Copy, Debug)]
enum Tier {
    /// New and out of the map: no paint rows (the row invariant), or childless with every row off-surface, which keeps a zoomed-out canvas from filling the map. [`Tier::SubtreeMoved`]'s insert leg repays the latter when a move brings rows on-surface.
    Untracked,
    /// No snapshot; everything this node paints is new.
    Added,
    /// Authoring, cascade state and parent all match, so by `subtree_hash` induction every descendant is bit-identical and the walk jumps the subtree. An idle frame takes this at the root.
    SubtreeUnchanged,
    /// Authoring and parent match but `cascade_input` moved (scroll, pan, sibling shift). Rows are unchanged, so damage is the subtree's prior extent ∪ current one, not the per-row flood.
    SubtreeMoved,
    /// This node is unchanged; a descendant is not. Only the rollup refreshes.
    DescendantChanged,
    /// This node's own paints changed.
    PaintsChanged(NodeSnapshot),
    /// Painted last frame, paints nothing now.
    Evicted(NodeSnapshot),
}

/// One layer's structural diff, built per layer so mutable diff state is reborrowed from `DamageEngine`.
#[derive(Debug)]
pub(super) struct LayerWalk<'a> {
    pub(super) prev: &'a mut WidgetIdMap<NodeSnapshot>,
    pub(super) paints: &'a mut BlockArena<Paint>,
    pub(super) matcher: &'a mut RowMatcher,
    pub(super) raw_rects: &'a mut Vec<Rect>,
    /// Per-row screen extents for the order-inversion check; filled only when a node's row order inverted.
    pub(super) order_extents: &'a mut Vec<Rect>,
    pub(super) inversions: &'a mut InvertedOverlaps,
    pub(super) counters: &'a mut DamageCounters,
    pub(super) surface: Rect,
    /// On a force-full frame the caller discards the region, so arms skip rect pushes.
    pub(super) force_full: bool,
    pub(super) layer: Layer,
    pub(super) tree: &'a Tree,
    pub(super) cascade: &'a LayerCascade,
}

impl LayerWalk<'_> {
    pub(super) fn run(&mut self) {
        let n = self.tree.records.len();
        let mut i = 0;
        while i < n {
            let parent_key = self.parent_key(i);
            let wid = self.tree.records.widget_id()[i];
            let advance = match self.classify(i, wid, parent_key) {
                Tier::Untracked => 1,
                Tier::Added => self.on_added(i, wid, parent_key),
                Tier::SubtreeUnchanged => self.on_subtree_unchanged(i),
                Tier::SubtreeMoved => self.on_subtree_moved(i),
                Tier::DescendantChanged => self.on_descendant_changed(i, wid),
                Tier::PaintsChanged(prev) => self.on_paints_changed(i, wid, prev, parent_key),
                Tier::Evicted(prev) => self.on_evicted(i, wid, prev),
            };
            i += advance;
        }
    }

    fn snapshot(&self, i: usize, parent_key: u64, paint_span: Span) -> NodeSnapshot {
        NodeSnapshot {
            paint_span,
            hash: self.tree.rollups.node[i],
            subtree_hash: self.tree.rollups.subtree[i],
            cascade_input: self.cascade.cascade_inputs[i],
            parent_key,
        }
    }

    /// The `parent_key` node `i` sits under: its parent's `WidgetId` bits, or the layer discriminant for a root, so a subtree changing layers cannot read as unchanged.
    fn parent_key(&self, i: usize) -> u64 {
        match self.tree.parent_of(i) {
            Some(parent) => self.tree.records.widget_id()[parent.idx()].0,
            None => self.layer as u64,
        }
    }

    /// The map read is the classification; writing arms re-probe the bucket. Deliberate and cheap: `prev` hashes by identity.
    fn classify(&self, i: usize, wid: WidgetId, parent_key: u64) -> Tier {
        let Some(prev) = self.prev.get(&wid).copied() else {
            let rows = self.cascade.paint_arena.rows_of(i);
            if rows.is_empty() {
                return Tier::Untracked;
            }
            return if !self.tree.has_children(i) && !rows.any_on_surface(self.surface) {
                Tier::Untracked
            } else {
                Tier::Added
            };
        };
        // A reparent or layer move keeps every hash but flips compositing order, so it disqualifies every skip tier.
        let same_parent = prev.parent_key == parent_key;
        let same_subtree = prev.subtree_hash == self.tree.rollups.subtree[i];
        let same_cascade = prev.cascade_input == self.cascade.cascade_inputs[i];
        match () {
            () if same_parent && same_subtree && same_cascade => Tier::SubtreeUnchanged,
            () if same_parent && same_subtree => Tier::SubtreeMoved,
            () if same_parent && same_cascade && prev.hash == self.tree.rollups.node[i] => {
                Tier::DescendantChanged
            }
            () if self.cascade.paint_arena.rows_of(i).is_empty() => Tier::Evicted(prev),
            () => Tier::PaintsChanged(prev),
        }
    }

    fn on_added(&mut self, i: usize, wid: WidgetId, parent_key: u64) -> usize {
        let rows = self.cascade.paint_arena.rows_of(i);
        let paint_span = self.paints.store(rows);
        if !self.force_full {
            self.raw_rects.extend(rows.screens());
        }
        let snapshot = self.snapshot(i, parent_key, paint_span);
        self.prev.insert(wid, snapshot);
        self.counters.mark_dirty(NodeId(i as u32));
        1
    }

    fn on_subtree_unchanged(&mut self, i: usize) -> usize {
        let span = self.tree.subtree_end_of(i) - i;
        self.counters.subtree_skipped(span);
        span
    }

    fn on_descendant_changed(&mut self, i: usize, wid: WidgetId) -> usize {
        // `classify` read this snapshot first, so the bucket exists; a miss would leave a stale subtree hash forever.
        let snap = self
            .prev
            .get_mut(&wid)
            .expect("DescendantChanged is classified from this node's own snapshot");
        snap.subtree_hash = self.tree.rollups.subtree[i];
        1
    }

    fn on_evicted(&mut self, i: usize, wid: WidgetId, prev: NodeSnapshot) -> usize {
        // Rows → rowless: push everything the node was painting, then drop it.
        self.raw_rects
            .extend(self.paints.slots[prev.paint_span.range()].screens());
        self.prev.remove(&wid);
        self.paints.release(prev.paint_span);
        self.counters.mark_dirty(NodeId(i as u32));
        1
    }

    fn on_paints_changed(
        &mut self,
        i: usize,
        wid: WidgetId,
        prev: NodeSnapshot,
        parent_key: u64,
    ) -> usize {
        let node = NodeId(i as u32);
        let curr = self.cascade.paint_arena.rows_of(i);
        let leg = self
            .matcher
            .diff_changed_leg(self.paints, self.raw_rects, prev.paint_span, curr);

        // Exact-matched rows emit no content damage, but a pair whose paint order inverted still flips its overlap. Only exact pairs participate; moved and added rows already cover theirs.
        if leg.order_inverted {
            self.emit_inverted_overlaps(node);
        }

        // Any `cascade_input` change alters pixels of exact-matched rows, so the union repaints on every flip. A pure `node_hash` flip (a child added or removed) is covered by the subtree diff; the union there would re-damage every direct shape.
        if prev.cascade_input != self.cascade.cascade_inputs[i] {
            let union = self.cascade.paint_arena.rows_of(i).union_screens();
            damage::push_screen(self.raw_rects, union);
        }

        // Reparent / layer move at identical content: damage the subtree's current extent; descendants keep their skip.
        if prev.parent_key != parent_key {
            let extent = self.cascade.subtree_paint_rects[i];
            damage::push_screen(self.raw_rects, extent);
        }

        let snapshot = self.snapshot(i, parent_key, leg.span);
        self.prev.insert(wid, snapshot);
        self.counters.mark_dirty(NodeId(i as u32));
        1
    }

    /// Tier [`Tier::SubtreeMoved`]: damage the union of what the subtree painted and paints now, then re-baseline every node in it.
    ///
    /// Equal `subtree_hash` pins each painting node's row count, so the in-place `copy_from_slice` is sound and only `cascade_input` refreshes. A node the cascade made invisible has no rows and is evicted here. A node with no snapshot is inserted when a move brings its rows on-surface.
    fn on_subtree_moved(&mut self, i: usize) -> usize {
        let end = self.tree.subtree_end_of(i);
        let mut prev_extent = Rect::ZERO;
        for j in i..end {
            let wid = self.tree.records.widget_id()[j];
            let curr = self.cascade.paint_arena.rows_of(j);
            if curr.is_empty() {
                if let Some(snap) = self.prev.remove(&wid) {
                    prev_extent = prev_extent
                        .union(self.paints.slots[snap.paint_span.range()].union_screens());
                    self.paints.release(snap.paint_span);
                    self.counters.mark_dirty(NodeId(j as u32));
                }
                continue;
            }
            if let Some(snap) = self.prev.get_mut(&wid) {
                snap.cascade_input = self.cascade.cascade_inputs[j];
                let paint_span = snap.paint_span;
                prev_extent =
                    prev_extent.union(self.paints.slots[paint_span.range()].union_screens());
                self.paints.slots[paint_span.range()].copy_from_slice(curr);
            } else {
                if !curr.any_on_surface(self.surface) {
                    continue;
                }
                let paint_span = self.paints.store(curr);
                let snapshot = self.snapshot(j, self.parent_key(j), paint_span);
                self.prev.insert(wid, snapshot);
            }
            self.counters.mark_dirty(NodeId(j as u32));
        }
        damage::push_screen(self.raw_rects, prev_extent);
        // Rolled-up curr extent from the cascade; `Rect::ZERO` for an invisible subtree. The prev half cannot use the column, since last frame's is gone.
        let curr_extent = self.cascade.subtree_paint_rects[i];
        damage::push_screen(self.raw_rects, curr_extent);
        end - i
    }

    /// Damage the overlap of every exact-matched row pair whose paint order inverted. Reached only behind [`RowMatcher::has_order_inversion`](crate::damage::row_matcher::RowMatcher::has_order_inversion).
    fn emit_inverted_overlaps(&mut self, node: NodeId) {
        self.build_row_extents(node);
        self.inversions.push(
            self.raw_rects,
            self.matcher.matched_positions(),
            self.order_extents,
        );
    }

    /// Screen extent per row of `node`'s paint span: chrome and shapes keep their `Paint.screen`; a child marker's zero rect becomes its subtree's painted extent. One cursor advances across rows and the `TreeItems` stream.
    fn build_row_extents(&mut self, node: NodeId) {
        let arena = &self.cascade.paint_arena;
        let node_span = arena.node_spans[node.idx()];
        self.order_extents.clear();
        let mut row = node_span.start as usize;
        if self.tree.chrome(node).is_some() {
            self.order_extents.push(arena.rows[row].screen);
            row += 1;
        }
        for item in self.tree.tree_items(node) {
            let extent = match item {
                TreeItem::ShapeRecord(..) => arena.rows[row].screen,
                TreeItem::Child(child) => self.cascade.subtree_paint_rects[child.id.idx()],
            };
            row += 1;
            self.order_extents.push(extent);
        }
        debug_assert_eq!(
            self.order_extents.len(),
            node_span.len as usize,
            "row extents out of sync with the owner's paint span",
        );
    }
}
