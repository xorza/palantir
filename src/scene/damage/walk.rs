//! The per-layer structural diff: [`LayerWalk`] and the [`Tier`] it
//! sorts each node into.
//!
//! Split out of `DamageEngine::compute`, which held the whole thing in
//! one body and paid for it three ways: six mutable fields hand-aliased
//! into locals to keep the borrow checker happy, a `usize::MAX` sentinel
//! smuggling one arm's work past the `Entry` borrow that arm couldn't
//! release, and three separate copies of the parent-stack push/pop.
//!
//! All three came from the same root cause — the tier decision and the
//! tier's *work* were fused into one `match` on a live `Entry`.
//! [`NodeSnapshot`] is `Copy`, so [`LayerWalk::classify`] reads a copy
//! and hands back a plain [`Tier`]; by the time any arm runs, nothing is
//! borrowed and each arm is an ordinary `&mut self` method.

use crate::common::block_arena::BlockArena;
use crate::primitives::rect::Rect;
use crate::primitives::span::Span;
use crate::primitives::widget_id::{WidgetId, WidgetIdMap};
use crate::scene::cascade::LayerCascade;
use crate::scene::cascade::paint::{Paint, PaintRows};
use crate::scene::damage::counters::DamageCounters;
use crate::scene::damage::node_snapshot::NodeSnapshot;
use crate::scene::damage::push_screen;
use crate::scene::damage::row_matcher::{ROW_UNMATCHED, RowMatcher};
use crate::scene::layer::Layer;
use crate::scene::tree::Tree;
use crate::scene::tree::iter::TreeItem;
use crate::scene::tree::node_id::NodeId;

/// Damage the overlap of every pair of rows whose relative paint order
/// flipped, given the pairing [`RowMatcher::diff_changed_leg`] left and
/// the per-row extents [`LayerWalk::build_row_extents`] resolved.
///
/// A free function over its two inputs so the pair enumeration can be
/// measured and tested without a tree or a cascade behind it.
fn emit_inverted_overlaps_into(out: &mut Vec<Rect>, matched: &[u32], extents: &[Rect]) {
    for j2 in 1..matched.len() {
        let p2 = matched[j2];
        if p2 == ROW_UNMATCHED {
            continue;
        }
        for (j1, &p1) in matched.iter().enumerate().take(j2) {
            if p1 == ROW_UNMATCHED || p1 < p2 {
                continue;
            }
            push_screen(out, extents[j1].clamp_to(extents[j2]));
        }
    }
}

/// One open ancestor on the diff walk's parent stack.
#[derive(Clone, Copy, Debug)]
pub(super) struct ParentFrame {
    /// Pre-order index one past the ancestor's subtree — popped once the
    /// walk reaches it.
    end: u32,
    /// The ancestor's `WidgetId` bits — the `parent_key` of every node
    /// directly under it.
    key: u64,
}

/// What the diff decided about one node, before it did anything about
/// it. Ordered cheapest-and-most-common first; [`LayerWalk::classify`]
/// is the single place that ordering is written down.
#[derive(Clone, Copy, Debug)]
enum Tier {
    /// New, childless, and painting nothing on-surface. Deliberately
    /// left out of the map: a zoomed-out canvas must not fill it with
    /// thousands of never-visible snapshots. The omission is repaid by
    /// [`Tier::SubtreeMoved`]'s insert leg the frame a move brings the
    /// rows on-surface.
    Untracked,
    /// No snapshot — everything this node paints is new.
    Added,
    /// Authoring, cascade state and parent all match. `subtree_hash`
    /// rolls up this node's own `node_hash`, so by induction every
    /// descendant is bit-identical and the walk jumps the subtree. The
    /// dominant steady-state path: an idle frame takes this at the root
    /// and does nothing else.
    SubtreeUnchanged,
    /// Authoring and parent match but `cascade_input` moved — a scroll
    /// tick, a pan, a sibling shift. Same widgets, same rows, same row
    /// hashes; only their screens differ, so damage is exactly "what the
    /// subtree painted before ∪ what it paints now" rather than the
    /// per-row matcher's two-rects-per-row flood.
    SubtreeMoved,
    /// This node is unchanged; a descendant is not. Its own arena rows
    /// stay correct, so only the rollup needs refreshing.
    DescendantChanged,
    /// This node's own paints changed.
    PaintsChanged(NodeSnapshot),
    /// Painted last frame, paints nothing now.
    Evicted(NodeSnapshot),
}

/// One layer's structural diff.
///
/// Built fresh per layer so the mutable diff state is reborrowed from
/// `DamageEngine` each time. Every field the arms mutate is a field
/// here, which is what lets them be methods: disjoint field borrows do
/// the work the hand-written local aliases used to.
#[derive(Debug)]
pub(super) struct LayerWalk<'a> {
    pub(super) prev: &'a mut WidgetIdMap<NodeSnapshot>,
    pub(super) paints: &'a mut BlockArena<Paint>,
    pub(super) matcher: &'a mut RowMatcher,
    pub(super) raw_rects: &'a mut Vec<Rect>,
    /// Per-row screen extents for the order-inversion check. Only filled
    /// on the rare frame a node's row order actually inverted.
    pub(super) order_extents: &'a mut Vec<Rect>,
    pub(super) parents: &'a mut Vec<ParentFrame>,
    pub(super) probe: &'a mut DamageCounters,
    pub(super) surface: Rect,
    /// On a force-full frame the caller discards the region, so the arms
    /// skip their rect pushes — a resize storm does no rect work, and
    /// `raw_rects`' retained capacity tracks real incremental frames
    /// rather than the whole tree.
    pub(super) force_full: bool,
    pub(super) layer: Layer,
    pub(super) tree: &'a Tree,
    pub(super) cascade: &'a LayerCascade,
}

impl LayerWalk<'_> {
    pub(super) fn run(&mut self) {
        self.parents.clear();
        let n = self.tree.records.len();
        let mut i = 0;
        while i < n {
            let parent_key = self.parent_key_at(i);
            // Loaded once and handed down: it is what every tier below
            // keys on, and the column read was repeated in each of them.
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
            // Descending into children opens a parent frame. A subtree
            // jump doesn't: `SubtreeUnchanged` never visits them, and
            // `SubtreeMoved` opened its own along the way.
            if advance == 1 {
                self.open_parent(i);
            }
            i += advance;
        }
    }

    // ---- the columns this layer walks -------------------------------

    fn rows(&self, i: usize) -> &[Paint] {
        let span = self.cascade.paint_arena.node_spans[i];
        &self.cascade.paint_arena.rows[span.range()]
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

    // ---- the one parent stack ---------------------------------------

    /// The `parent_key` node `i` sits under, retiring any ancestor whose
    /// subtree the walk has passed.
    ///
    /// Roots key on the layer discriminant, so a subtree migrating
    /// between layers can't read as unchanged. Both the per-node walk
    /// and the moved-subtree jump go through here, sharing **one** stack:
    /// the jump's frames all end within the jumped subtree, so the next
    /// outer `parent_key_at` retires them on its own. Two stacks — the
    /// jump riding the tail of the outer one — would need a `jump_base`
    /// truncation to do the same job.
    fn parent_key_at(&mut self, i: usize) -> u64 {
        while self.parents.last().is_some_and(|f| i as u32 >= f.end) {
            self.parents.pop();
        }
        self.parents
            .last()
            .map_or(self.layer as u64, |frame| frame.key)
    }

    /// Open `i` as the enclosing parent for the nodes that follow it,
    /// if it has any.
    fn open_parent(&mut self, i: usize) {
        let end = self.tree.subtree_end_of(i);
        if end > i + 1 {
            self.parents.push(ParentFrame {
                end: end as u32,
                key: self.tree.records.widget_id()[i].0,
            });
        }
    }

    // ---- classification ---------------------------------------------

    /// The map read here *is* the classification; the arms that write probe
    /// the same bucket again. Deliberate — see the module doc for what
    /// holding a live `Entry` across the tier dispatch cost — and cheap:
    /// `prev` hashes by identity, so the repeat is a touch on the line the
    /// classification just pulled in.
    fn classify(&self, i: usize, wid: WidgetId, parent_key: u64) -> Tier {
        let Some(prev) = self.prev.get(&wid).copied() else {
            let childless = self.tree.subtree_end_of(i) == i + 1;
            return if childless && !self.rows(i).any_on_surface(self.surface) {
                Tier::Untracked
            } else {
                Tier::Added
            };
        };
        // A reparent or layer move keeps every hash — `cascade_input`
        // folds ancestor *state*, not identity — yet flips compositing
        // order against outside overlappers. So it disqualifies every
        // skip tier.
        let same_parent = prev.parent_key == parent_key;
        let same_subtree = prev.subtree_hash == self.tree.rollups.subtree[i];
        let same_cascade = prev.cascade_input == self.cascade.cascade_inputs[i];
        match () {
            _ if same_parent && same_subtree && same_cascade => Tier::SubtreeUnchanged,
            _ if same_parent && same_subtree => Tier::SubtreeMoved,
            _ if same_parent && same_cascade && prev.hash == self.tree.rollups.node[i] => {
                Tier::DescendantChanged
            }
            _ if self.rows(i).is_empty() => Tier::Evicted(prev),
            _ => Tier::PaintsChanged(prev),
        }
    }

    // ---- one method per tier ----------------------------------------

    fn on_added(&mut self, i: usize, wid: WidgetId, parent_key: u64) -> usize {
        let span = self.cascade.paint_arena.node_spans[i];
        let paint_span = self
            .paints
            .store(&self.cascade.paint_arena.rows[span.range()]);
        if !self.force_full {
            for screen in self.cascade.paint_arena.rows[span.range()].screens() {
                self.raw_rects.push(screen);
            }
        }
        let snapshot = self.snapshot(i, parent_key, paint_span);
        self.prev.insert(wid, snapshot);
        self.probe.mark_dirty(NodeId(i as u32));
        1
    }

    fn on_subtree_unchanged(&mut self, i: usize) -> usize {
        let span = self.tree.subtree_end_of(i) - i;
        self.probe.subtree_skipped(span);
        span
    }

    fn on_descendant_changed(&mut self, i: usize, wid: WidgetId) -> usize {
        // `classify` reaches this tier only after reading this node's
        // snapshot, so the bucket is there. Stated rather than skipped
        // past: a missing one would mean the walk lost a node between the
        // two lookups, and quietly declining to refresh the subtree hash
        // leaves the node reporting a stale one every frame after.
        let snap = self
            .prev
            .get_mut(&wid)
            .expect("DescendantChanged is classified from this node's own snapshot");
        snap.subtree_hash = self.tree.rollups.subtree[i];
        1
    }

    fn on_evicted(&mut self, i: usize, wid: WidgetId, prev: NodeSnapshot) -> usize {
        // Rows → rowless: push everything the node *was* painting, then
        // drop it.
        for screen in self.paints.slots[prev.paint_span.range()].screens() {
            self.raw_rects.push(screen);
        }
        self.prev.remove(&wid);
        self.paints.release(prev.paint_span);
        self.probe.mark_dirty(NodeId(i as u32));
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
        let span = self.cascade.paint_arena.node_spans[i];
        let curr = &self.cascade.paint_arena.rows[span.range()];
        let leg = self
            .matcher
            .diff_changed_leg(self.paints, self.raw_rects, prev.paint_span, curr);

        // Exact-matched rows emitted no content damage, but a pair whose
        // relative paint order inverted (a raised node, a shape crossing
        // a child boundary, two coincident shapes swapping) still flips
        // its overlap's pixels. Moved and added rows already pushed full
        // rects covering any overlap they sit in, so only exact pairs
        // participate.
        if leg.order_inverted {
            self.emit_inverted_overlaps(node);
        }

        // A `cascade_input` change (ancestor disable, clip-saturated
        // pan, visibility toggle) alters pixels of rows the per-shape
        // diff matched exactly and emitted nothing for — a hidden node's
        // untouched shapes must still clear. So the union repaints on
        // any `cascade_input` flip, *including* frames where some row
        // also changed: gating on geometry left the exact-matched rows
        // undamaged when a visibility flip landed on the same frame as a
        // mid-tween shape. A pure `node_hash` flip with unchanged
        // `cascade_input` means the authoring stream differed without
        // touching own pixels — most commonly a child added or removed,
        // already covered by the subtree/eviction diff. Repainting the
        // union there would spuriously re-damage every direct shape,
        // e.g. all canvas connections when an unrelated node is deleted.
        let union = self.rows(i).union_screens();
        if prev.cascade_input != self.cascade.cascade_inputs[i] && !union.is_paint_empty() {
            self.raw_rects.push(union);
        }

        // Reparent / layer move at otherwise-identical content: the
        // whole subtree moved together, so damage its current painted
        // extent. Descendants keep their skip — their snapshots are
        // intact and this push already covers them.
        if prev.parent_key != parent_key {
            let extent = self
                .cascade
                .paint_arena
                .subtree_extent(node, self.tree.records.subtree_end());
            if !extent.is_paint_empty() {
                self.raw_rects.push(extent);
            }
        }

        let snapshot = self.snapshot(i, parent_key, leg.span);
        self.prev.insert(wid, snapshot);
        self.probe.mark_dirty(NodeId(i as u32));
        1
    }

    /// Tier [`Tier::SubtreeMoved`]: damage the union of what the subtree
    /// painted and what it paints now, then re-baseline every node in it
    /// without re-deriving anything the induction already gives us.
    ///
    /// Equal `subtree_hash` pins the row *count* per node, which is what
    /// makes the in-place `copy_from_slice` below sound: each snapshot
    /// keeps the block it already holds, so no span moves and only
    /// `cascade_input` needs refreshing.
    /// A node with no snapshot was skipped as [`Tier::Untracked`] back
    /// when it painted nothing visible; the frame a move brings its rows
    /// on-surface it gets inserted here, which is what keeps every node
    /// painting visible pixels in the map for later prev-extent folds
    /// and for the removed-widget eviction tail.
    fn on_subtree_moved(&mut self, i: usize) -> usize {
        let end = self.tree.subtree_end_of(i);
        // Seeded, like the curr extent read below it: both fold through
        // `Rect::union`'s identity, so a subtree that painted nothing
        // comes out `Rect::ZERO` on either side and the two arms of this
        // one test read the same way.
        let mut prev_extent = Rect::ZERO;
        for j in i..end {
            // Same stack as the outer walk: at `j == i` nothing is
            // retired and this reads `i`'s own parent.
            let j_parent_key = self.parent_key_at(j);
            self.open_parent(j);
            let span = self.cascade.paint_arena.node_spans[j];
            if span.len == 0 {
                continue;
            }
            let wid = self.tree.records.widget_id()[j];
            // One probe: the refresh below is the only write, so it takes the
            // `&mut` up front rather than reading the snapshot and coming back
            // for the same bucket — this runs per moved node, which is every
            // node under a pan.
            match self.prev.get_mut(&wid) {
                Some(snap) => {
                    snap.cascade_input = self.cascade.cascade_inputs[j];
                    let paint_span = snap.paint_span;
                    prev_extent =
                        prev_extent.union(self.paints.slots[paint_span.range()].union_screens());
                    let curr = &self.cascade.paint_arena.rows[span.range()];
                    self.paints.slots[paint_span.range()].copy_from_slice(curr);
                }
                None => {
                    let curr = &self.cascade.paint_arena.rows[span.range()];
                    if !curr.any_on_surface(self.surface) {
                        continue;
                    }
                    let paint_span = self.paints.store(curr);
                    let snapshot = self.snapshot(j, j_parent_key, paint_span);
                    self.prev.insert(wid, snapshot);
                }
            }
            self.probe.mark_dirty(NodeId(j as u32));
        }
        if !prev_extent.is_paint_empty() {
            self.raw_rects.push(prev_extent);
        }
        // Rolled-up curr extent from the cascade — already `Rect::ZERO`
        // for invisible subtrees, so a hide transition damages only the
        // prev pixels.
        //
        // Off the column where the prev half came off the retained rows,
        // because that is what each side has: last frame's column is
        // gone, and this frame's rows are the same fold the column
        // already carries. The two differ on one node — a chromeless
        // clip-only container contributes its own visible rect to the
        // column and no row to the fold — and that difference is on the
        // side that can afford it: the curr push may cover more than the
        // subtree paints, the prev push may not cover less.
        let curr_extent = self.cascade.subtree_paint_rects[i];
        if !curr_extent.is_paint_empty() {
            self.raw_rects.push(curr_extent);
        }
        end - i
    }

    // ---- paint-order inversion --------------------------------------

    /// Damage the extent overlap of every exact-matched row pair whose
    /// relative paint order inverted since last frame.
    ///
    /// `O(rows²)` pair enumeration, reached only behind
    /// [`RowMatcher::has_order_inversion`](crate::scene::damage::row_matcher::RowMatcher::has_order_inversion) on the rare frame an order actually
    /// flipped. Rows that merely shifted because a sibling was added or
    /// removed keep their relative order and contribute nothing.
    /// [`push_screen`] drops degenerate results — a zero-size extent
    /// pinned strictly inside a sibling does pass `intersects`, and a
    /// sub-EPS overlap sliver paints nothing; neither earns a merge slot.
    fn emit_inverted_overlaps(&mut self, node: NodeId) {
        self.build_row_extents(node);
        emit_inverted_overlaps_into(
            self.raw_rects,
            self.matcher.matched_positions(),
            self.order_extents,
        );
    }

    /// Screen-space extent per row of `node`'s paint span, in row order:
    /// chrome and direct shapes keep their own `Paint.screen`; a child
    /// marker's zero rect is swapped for its subtree's painted extent —
    /// the pixels that actually move when the child's paint order flips.
    ///
    /// Rows are 1:1 with chrome + the node's `TreeItems` stream, since
    /// the cascade emits them from the same walk, so one cursor advances
    /// across both.
    fn build_row_extents(&mut self, node: NodeId) {
        let arena = &self.cascade.paint_arena;
        let node_span = arena.node_spans[node.idx()];
        let subtree_end = self.tree.records.subtree_end();
        self.order_extents.clear();
        let mut row = node_span.start as usize;
        if self.tree.chrome(node).is_some() {
            self.order_extents.push(arena.rows[row].screen);
            row += 1;
        }
        for item in self.tree.tree_items(node) {
            // Every item — shape *and* child marker — owns one arena
            // row, so the cursor advances on both. An explicit `row`
            // rather than `node_span.start + out.len()`: the latter makes
            // the output vector's length double as the read cursor.
            let extent = match item {
                TreeItem::ShapeRecord(..) => arena.rows[row].screen,
                TreeItem::Child(child) => arena.subtree_extent(child.id, subtree_end),
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
