//! Cross-frame measure cache with one whole-tree snapshot per frame.
//! Measure reads only from the previous snapshot while the completed
//! current layout is materialized once in pre-order. Each node, grid
//! hug value, and shaped text run is therefore retained exactly once.

#[cfg(feature = "bench")]
pub(crate) mod bench;

use crate::common::content_hash::ContentHash;
use crate::common::counters::BenchOnly;
use crate::common::span::Span;
use crate::layout::drivers::grid::grid_track_store::GridTrackStore;
use crate::layout::intrinsic::len_req::SLOT_COUNT;
use crate::layout::measured::Measured;
use crate::layout::text::shaped_text::ShapedText;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::geometry::size::Size;
use crate::primitives::identity::widget_id::{WidgetId, WidgetIdMap};
use crate::primitives::layout::layout_mode::LayoutMode;
use crate::primitives::math::num::F32Px;
use crate::scene::forest::Forest;
use crate::scene::layer::Layer;
use crate::scene::tree::Tree;
use glam::{IVec2, Vec2};
use std::mem;

#[derive(Clone, Copy, Debug)]
struct ArenaSnapshot {
    subtree_hash: ContentHash,
    available_q: AvailableKey,
    nodes: Span,
    tracks: Span,
    text_shapes: Span,
}

pub(super) type AvailableKey = IVec2;

pub(super) const INVALID_AVAILABLE: AvailableKey = IVec2::splat(i32::MIN);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct RootSnapshotKey {
    pub(super) wid: WidgetId,
    pub(super) subtree_hash: ContentHash,
    pub(super) available_q: AvailableKey,
}

#[derive(Debug)]
pub(super) struct CachedSubtree<'a> {
    pub(super) root: Measured,
    /// Arena index this subtree's columns start at. `arrange` stores it
    /// per node so it can re-slice `rect` without a second map probe.
    pub(super) nodes_base: u32,
    pub(super) desired: &'a [Size],
    pub(super) floor: &'a [Size],
    pub(super) stable_from: &'a [Size],
    pub(super) scroll_content: &'a [Size],
    pub(super) text_spans: &'a [Span],
    pub(super) intrinsics: &'a [[f32; SLOT_COUNT]],
    pub(super) available_q: &'a [AvailableKey],
    pub(super) tracks: &'a [f32],
    pub(super) text_shapes: &'a [ShapedText],
    pub(super) text_shapes_base: u32,
}

/// A subtree's arrange as last frame captured it: each node's rect, and
/// its slot origin in its parent's inner box.
#[derive(Debug)]
pub(super) struct Arranged<'a> {
    pub(super) rects: &'a [Rect],
    pub(super) locals: &'a [Vec2],
}

#[derive(Clone, Copy, Debug)]
pub(super) struct CaptureTreeInput<'a> {
    pub(super) desired: &'a [Size],
    pub(super) floor: &'a [Size],
    pub(super) stable_from: &'a [Size],
    pub(super) rect: &'a [Rect],
    pub(super) local: &'a [Vec2],
    pub(super) scroll_content: &'a [Size],
    pub(super) intrinsics: &'a [[f32; SLOT_COUNT]],
    pub(super) available_q: &'a [AvailableKey],
    pub(super) grid_track_state: &'a GridTrackStore,
    pub(super) text_spans: &'a [Span],
    pub(super) text_shapes: &'a [ShapedText],
}

fn union_spans(a: Span, b: Span) -> Span {
    if a.len == 0 {
        return b;
    }
    if b.len == 0 {
        return a;
    }
    let start = a.start.min(b.start);
    let end = (a.start + a.len).max(b.start + b.len);
    Span::new(start, end - start)
}

#[derive(Debug, Default)]
pub(crate) struct NodeArenas {
    desired: Vec<Size>,
    floor: Vec<Size>,
    stable_from: Vec<Size>,
    /// Arranged rect per node, captured after `arrange` wrote it. One of
    /// the two columns produced by the *second* half of the layout pass;
    /// `LayoutPass::arrange` replays it instead of re-running the
    /// drivers when a subtree's slot is unchanged or merely translated.
    rect: Vec<Rect>,
    /// Each node's slot origin in its parent's inner box — the other
    /// arrange column, and what a translated replay rebuilds `rect` from.
    local: Vec<Vec2>,
    scroll_content: Vec<Size>,
    text_spans: Vec<Span>,
    intrinsics: Vec<[f32; SLOT_COUNT]>,
    available_q: Vec<AvailableKey>,
}

impl NodeArenas {
    /// Destructured rather than a statement list: `node_base` is taken
    /// from `desired.len()`, so an arena left longer than the others
    /// offsets every later slice of that column by the leftover — stale
    /// rows from a previous frame, with nothing to assert on.
    fn clear(&mut self) {
        let Self {
            desired,
            floor,
            stable_from,
            rect,
            local,
            scroll_content,
            text_spans,
            intrinsics,
            available_q,
        } = self;
        desired.clear();
        floor.clear();
        stable_from.clear();
        rect.clear();
        local.clear();
        scroll_content.clear();
        text_spans.clear();
        intrinsics.clear();
        available_q.clear();
    }
}

#[derive(Debug, Default)]
pub(crate) struct MeasureSnapshot {
    nodes: NodeArenas,
    tracks: Vec<f32>,
    text_shapes: Vec<ShapedText>,
    snapshots: WidgetIdMap<u32>,
    descriptors: Vec<ArenaSnapshot>,
    descriptor_wids: Vec<WidgetId>,
    pub(super) roots: Vec<RootSnapshotKey>,
    /// Ordered fold over the `WidgetId`s in `descriptor_wids`, rebuilt
    /// from scratch by every capture.
    descriptor_identity: u64,
    /// The `descriptor_identity` [`Self::snapshots`] was last built for.
    ///
    /// Held beside the map so the two travel together through
    /// `end_frame`'s buffer swap. That is the whole trick: this
    /// snapshot's map is reusable exactly when the capture that just
    /// filled the same struct produced the same identity, so nothing has
    /// to reason about which frame the retained map came from.
    snapshots_identity: u64,
}

impl MeasureSnapshot {
    /// Empty everything, retained descriptor map included — see
    /// [`MeasureCache::forget_all`] for when a snapshot is thrown away
    /// whole.
    ///
    /// `snapshots_identity` goes with the map. Leaving it set would let
    /// the next capture that happens to fold to the same value reuse a
    /// map that is no longer there.
    fn forget_all(&mut self) {
        self.clear_capture();
        self.snapshots.clear();
        self.snapshots_identity = 0;
    }

    /// Empty the captured columns, keeping the retained descriptor map —
    /// the half a new capture refills. [`Self::forget_all`] is the
    /// whole-snapshot peer, and the names say which is which.
    fn clear_capture(&mut self) {
        self.nodes.clear();
        self.tracks.clear();
        self.text_shapes.clear();
        self.descriptors.clear();
        self.descriptor_wids.clear();
        self.roots.clear();
        self.descriptor_identity = 0;
    }

    /// Rebuild `snapshots` from `descriptor_wids`, unless the capture
    /// that just ran produced the very id sequence the retained map was
    /// already built for.
    ///
    /// Sequence equality is *approximated* by `descriptor_identity`; a
    /// collision would hand `try_lookup` a map pointing at another
    /// widget's descriptor. That is survivable — the `subtree_hash` check
    /// there rejects a mismatched descriptor, so the worst case is a
    /// missed hit — but it would be silent, so
    /// [`Self::snapshots_match_descriptors`] proves the approximation in
    /// debug builds instead of leaving it argued.
    /// Returns whether it rebuilt, which is the only thing distinguishing
    /// the two paths from outside — the map ends up correct either way.
    fn refresh_snapshots(&mut self) -> bool {
        if self.snapshots_identity == self.descriptor_identity {
            debug_assert!(
                self.snapshots_match_descriptors(),
                "descriptor_identity collided: the retained WidgetId → descriptor map \
                 disagrees with the descriptors just captured",
            );
            return false;
        }
        self.snapshots.clear();
        for (descriptor, wid) in self.descriptor_wids.iter().copied().enumerate() {
            self.snapshots.insert(wid, descriptor as u32);
        }
        self.snapshots_identity = self.descriptor_identity;
        true
    }

    /// Every captured descriptor is reachable through `snapshots` at its
    /// own index — precisely what reusing the map claims. One hash probe
    /// per descriptor, so it is only ever called from a `debug_assert!`.
    fn snapshots_match_descriptors(&self) -> bool {
        self.snapshots.len() == self.descriptor_wids.len()
            && self
                .descriptor_wids
                .iter()
                .copied()
                .enumerate()
                .all(|(descriptor, wid)| {
                    self.snapshots.get(&wid).copied() == Some(descriptor as u32)
                })
    }
}

#[derive(Debug, Default)]
pub(crate) struct MeasureCache {
    previous: MeasureSnapshot,
    current: MeasureSnapshot,
    hug_offsets: Vec<u32>,
    text_bounds: Vec<Span>,
    /// Snapshot-map rebuilds run so far. Lets a test prove the reuse gate
    /// in [`Self::end_frame`] both fires and busts — without it the
    /// `debug_assert` guarding reuse passes vacuously on any frame that
    /// silently stopped reusing.
    ///
    /// Accumulates for the life of the cache rather than resetting per
    /// frame, so readers take a delta — the shape every counter over a
    /// pass that may not run takes, for the reason
    /// [`CascadeCounters`](crate::cascade::counters::CascadeCounters)
    /// states.
    pub(crate) snapshot_rebuilds: BenchOnly<u32>,
}

impl MeasureCache {
    /// The cache key for an available size: each axis quantized to the
    /// grid `try_lookup` matches on.
    ///
    /// `pub(crate)` for one caller outside `layout`:
    /// `text::wrap::tests::wrap_target_matches_cache_grid`, which pins the
    /// wrap width against this very grid.
    #[inline]
    pub(crate) fn available_key(s: Size) -> AvailableKey {
        debug_assert!(s.w >= 0.0 && s.h >= 0.0, "negative available: {s:?}");
        IVec2::new(s.w.quantize_px(), s.h.quantize_px())
    }

    pub(super) fn begin_frame(&mut self) {
        self.current.clear_capture();
    }

    /// Whether last frame's capture still describes `forest` at
    /// `surface` — same node and root counts, and every root still
    /// under the same widget id, subtree hash and quantized available.
    /// A `false` here is what makes the next `run` a rebuild.
    pub(super) fn matches_forest(&self, forest: &Forest, surface: Rect) -> bool {
        let snapshot = &self.previous;
        if snapshot.nodes.desired.len() != forest.total_nodes()
            || snapshot.roots.len() != forest.total_roots()
        {
            return false;
        }
        let mut root_index = 0;
        for layer in Layer::PAINT_ORDER {
            let tree = &forest.trees[layer];
            for slot in &tree.roots {
                let root = slot.first_node;
                let current = RootSnapshotKey {
                    wid: tree.records.widget_id()[root.idx()],
                    subtree_hash: tree.rollups.layout_subtree[root.idx()],
                    available_q: Self::available_key(slot.available(layer, surface)),
                };
                if snapshot.roots[root_index] != current {
                    return false;
                }
                root_index += 1;
            }
        }
        true
    }

    /// Last frame's arrange of a subtree of `len` nodes whose capture
    /// starts at `base` — the third snapshot read, beside
    /// [`Self::try_lookup`] and [`Self::lookup_root_intrinsic`], rather
    /// than an index into the arena from outside.
    pub(super) fn arranged(&self, base: usize, len: usize) -> Arranged<'_> {
        let nodes = base..base + len;
        Arranged {
            rects: &self.previous.nodes.rect[nodes.clone()],
            locals: &self.previous.nodes.local[nodes],
        }
    }

    /// Last frame's measure of `wid`'s subtree, if its authoring is
    /// unchanged and it holds under `available` — the offer it was
    /// measured at, to the key's grid, or any finite offer past its
    /// [`Measured::stable_from`], per axis.
    #[inline]
    pub(super) fn try_lookup(
        &self,
        wid: WidgetId,
        curr_hash: ContentHash,
        available: Size,
        available_q: AvailableKey,
    ) -> Option<CachedSubtree<'_>> {
        let descriptor = *self.previous.snapshots.get(&wid)? as usize;
        let snap = &self.previous.descriptors[descriptor];
        if snap.subtree_hash != curr_hash {
            return None;
        }
        let nodes = snap.nodes.range();
        let stable_from = self.previous.nodes.stable_from[nodes.start];
        let holds = |measured_at: i32, key: i32, offer: f32, from: f32| {
            measured_at == key || (offer.is_finite() && offer >= from)
        };
        if !(holds(
            snap.available_q.x,
            available_q.x,
            available.w,
            stable_from.w,
        ) && holds(
            snap.available_q.y,
            available_q.y,
            available.h,
            stable_from.h,
        )) {
            return None;
        }
        Some(CachedSubtree {
            root: Measured {
                size: self.previous.nodes.desired[nodes.start],
                floor: self.previous.nodes.floor[nodes.start],
                stable_from,
            },
            nodes_base: snap.nodes.start,
            desired: &self.previous.nodes.desired[nodes.clone()],
            floor: &self.previous.nodes.floor[nodes.clone()],
            stable_from: &self.previous.nodes.stable_from[nodes.clone()],
            scroll_content: &self.previous.nodes.scroll_content[nodes.clone()],
            text_spans: &self.previous.nodes.text_spans[nodes.clone()],
            intrinsics: &self.previous.nodes.intrinsics[nodes.clone()],
            available_q: &self.previous.nodes.available_q[nodes],
            tracks: &self.previous.tracks[snap.tracks.range()],
            text_shapes: &self.previous.text_shapes[snap.text_shapes.range()],
            text_shapes_base: snap.text_shapes.start,
        })
    }

    #[inline]
    pub(super) fn lookup_root_intrinsic(
        &self,
        wid: WidgetId,
        subtree_hash: ContentHash,
        slot: usize,
    ) -> Option<f32> {
        let descriptor = *self.previous.snapshots.get(&wid)? as usize;
        let snap = &self.previous.descriptors[descriptor];
        if snap.subtree_hash != subtree_hash {
            return None;
        }
        let value = self.previous.nodes.intrinsics[snap.nodes.start as usize][slot];
        (!value.is_nan()).then_some(value)
    }

    pub(super) fn capture_tree(&mut self, tree: &Tree, input: CaptureTreeInput<'_>) {
        let CaptureTreeInput {
            desired,
            floor,
            stable_from,
            rect,
            local,
            scroll_content,
            intrinsics,
            available_q,
            grid_track_state,
            text_spans,
            text_shapes,
        } = input;
        let node_count = tree.records.len();
        // Column-length agreement is an engine invariant, not input
        // validation, and this runs once per layer per frame — debug only.
        debug_assert_eq!(desired.len(), node_count);
        debug_assert_eq!(floor.len(), node_count);
        debug_assert_eq!(stable_from.len(), node_count);
        debug_assert_eq!(rect.len(), node_count);
        debug_assert_eq!(local.len(), node_count);
        debug_assert_eq!(scroll_content.len(), node_count);
        debug_assert_eq!(intrinsics.len(), node_count);
        debug_assert_eq!(available_q.len(), node_count);
        debug_assert_eq!(text_spans.len(), node_count);

        let node_base = self.current.nodes.desired.len() as u32;
        let text_base = self.current.text_shapes.len() as u32;

        self.current.nodes.rect.extend_from_slice(rect);
        self.current.nodes.local.extend_from_slice(local);
        self.current.nodes.intrinsics.extend_from_slice(intrinsics);
        self.current
            .nodes
            .scroll_content
            .extend_from_slice(scroll_content);
        let has_text = !text_shapes.is_empty();
        if has_text {
            self.current.text_shapes.extend_from_slice(text_shapes);
            // Bare `resize`, no `clear` first: the loop below writes
            // every slot, so the fill value is irrelevant and clearing
            // would only add a truncate-then-regrow round trip at a
            // steady tree size. Same convention as
            // `SubtreeRollups::reset_for`.
            self.text_bounds.resize(node_count, Span::default());
            let mut owned_text_count = 0u32;
            for (index, span) in text_spans.iter().copied().enumerate() {
                // Both arms assign, which is what makes the column
                // total and lets the pass run without a preceding reset.
                // Leaving the text-less arm to inherit a default would
                // let a longer previous tree's bound leak through.
                let stored = if span.len == 0 {
                    Span::default()
                } else {
                    owned_text_count += span.len;
                    Span::new(text_base + span.start, span.len)
                };
                self.current.nodes.text_spans.push(stored);
                self.text_bounds[index] = stored;
            }
            debug_assert_eq!(owned_text_count as usize, text_shapes.len());

            for index in (0..node_count).rev() {
                let end = tree.subtree_end_of(index);
                let mut bound = self.text_bounds[index];
                let mut run_count = bound.len;
                let mut child = index + 1;
                while child < end {
                    let child_bound = self.text_bounds[child];
                    run_count += child_bound.len;
                    bound = union_spans(bound, child_bound);
                    child = tree.subtree_end_of(child);
                }
                debug_assert_eq!(
                    bound.len, run_count,
                    "a measured subtree's text runs must be contiguous"
                );
                self.text_bounds[index] = bound;
            }
        } else {
            self.current.nodes.text_spans.resize(
                self.current.nodes.text_spans.len() + node_count,
                Span::default(),
            );
        }

        let layouts = tree.records.layout();
        let has_grids = !tree.grid_defs.is_empty();
        if has_grids {
            // Every slot `0..=node_count` is assigned below, so this
            // wants the length, not the zeroes.
            self.hug_offsets.resize(node_count + 1, 0);
            for (index, layout) in layouts.iter().copied().enumerate() {
                self.hug_offsets[index] = self.current.tracks.len() as u32;
                if let LayoutMode::Grid(idx) = LayoutMode::from(layout.meta) {
                    grid_track_state.snapshot_grid(idx, &mut self.current.tracks);
                }
            }
            self.hug_offsets[node_count] = self.current.tracks.len() as u32;
        }

        for slot in &tree.roots {
            let index = slot.first_node.idx();
            self.current.roots.push(RootSnapshotKey {
                wid: tree.records.widget_id()[index],
                subtree_hash: tree.rollups.layout_subtree[index],
                available_q: available_q[index],
            });
        }

        for index in 0..node_count {
            if LayoutMode::from(layouts[index].meta) == LayoutMode::Leaf
                || available_q[index] == INVALID_AVAILABLE
            {
                continue;
            }
            let end = tree.subtree_end_of(index);
            let tracks = if has_grids {
                let start = self.hug_offsets[index];
                Span::new(start, self.hug_offsets[end] - start)
            } else {
                Span::default()
            };
            let text_shapes = if has_text {
                self.text_bounds[index]
            } else {
                Span::default()
            };
            let wid = tree.records.widget_id()[index];
            self.current.descriptor_identity = (self.current.descriptor_identity.rotate_left(5)
                ^ wid.0)
                .wrapping_mul(0x517c_c1b7_2722_0a95);
            self.current.descriptors.push(ArenaSnapshot {
                subtree_hash: tree.rollups.layout_subtree[index],
                available_q: available_q[index],
                nodes: Span::new(node_base + index as u32, (end - index) as u32),
                tracks,
                text_shapes,
            });
            self.current.descriptor_wids.push(wid);
        }

        // Copied for every layer, the first included, like the columns
        // above: a swap for the first would leave the engine's scratch
        // columns empty until the next `resize_for`, while the
        // container-text pass still runs.
        self.current.nodes.desired.extend_from_slice(desired);
        self.current.nodes.floor.extend_from_slice(floor);
        self.current
            .nodes
            .stable_from
            .extend_from_slice(stable_from);
        self.current
            .nodes
            .available_q
            .extend_from_slice(available_q);
    }

    pub(super) fn end_frame(&mut self) {
        if self.current.refresh_snapshots() {
            self.snapshot_rebuilds.bump();
        }
        mem::swap(&mut self.previous, &mut self.current);
    }

    /// Force a cold start: both buffers forget everything, so the next
    /// frame measures from scratch.
    ///
    /// **What every other invalidation here cannot express.** The
    /// snapshot checks all ask whether the *inputs* moved — the subtree
    /// hash, the available width — and the one event that changes what a
    /// run measures to while moving neither is a font load. See
    /// `TextSystem::sync_fonts`, which is what detects it.
    pub(super) fn forget_all(&mut self) {
        self.previous.forget_all();
        self.current.forget_all();
    }
}

/// What the measure-cache tests reach, and the adversarial tree shapes
/// the measure-cache bench times — shared with the tests that pin what
/// the cache retains for them: a deep chain and a balanced broad tree.
#[cfg(any(test, feature = "bench"))]
pub(crate) mod internals {
    #[cfg(test)]
    use super::*;
    use crate::primitives::layout::sizing::Sizing;
    use crate::ui::Ui;
    use crate::widget_core::configure::Configure;
    use crate::widgets::block::Block;
    use crate::widgets::panel::Panel;

    /// Nested panels in the deep chain, above its one leaf.
    pub(crate) const DEEP_DEPTH: usize = 192;
    /// Children per panel in the broad tree.
    pub(crate) const BROAD_FANOUT: usize = 8;
    /// Panel levels below the broad tree's root.
    pub(crate) const BROAD_DEPTH: usize = 3;

    pub(crate) fn build_deep(ui: &mut Ui) {
        build_deep_level(ui, 0);
    }

    fn build_deep_level(ui: &mut Ui, depth: usize) {
        if depth == DEEP_DEPTH {
            Block::new()
                .id_salt("deep-leaf")
                .size((Sizing::FILL, Sizing::fixed(1.0)))
                .show(ui);
            return;
        }

        Panel::vstack()
            .id_salt(("deep", depth))
            .size((Sizing::FILL, Sizing::HUG))
            .show(ui, |ui| build_deep_level(ui, depth + 1));
    }

    /// What a variant of the broad tree changes in its first leaf. Both
    /// are layout authoring: a colour would be paint, which the measure
    /// cache does not key on, and the whole tree would hit at the root.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub(crate) enum BroadChange {
        /// The leaf's fill weight, which an only child's geometry ignores.
        FillWeight,
        /// The leaf's height, which moves the minimum of every panel
        /// above it — and with it what each offers its children, once the
        /// tree is taller than its surface.
        LeafHeight,
    }

    pub(crate) fn build_broad(ui: &mut Ui) {
        build_broad_variant(ui, None);
    }

    pub(crate) fn build_broad_variant(ui: &mut Ui, change: Option<BroadChange>) {
        build_broad_level(ui, 0, 0, change);
    }

    fn build_broad_level(ui: &mut Ui, depth: usize, key: usize, change: Option<BroadChange>) {
        Panel::vstack()
            .id_salt(("broad", depth, key))
            .size((Sizing::FILL, Sizing::HUG))
            .show(ui, |ui| {
                if depth == BROAD_DEPTH {
                    let changed = |to| key == 0 && change == Some(to);
                    let weight = if changed(BroadChange::FillWeight) {
                        2.0
                    } else {
                        1.0
                    };
                    let height = if changed(BroadChange::LeafHeight) {
                        2.0
                    } else {
                        1.0
                    };
                    Block::new()
                        .id_salt(("broad-leaf", key))
                        .size((Sizing::fill(weight), Sizing::fixed(height)))
                        .show(ui);
                    return;
                }

                for child in 0..BROAD_FANOUT {
                    build_broad_level(ui, depth + 1, key * BROAD_FANOUT + child, change);
                }
            });
    }

    #[cfg(test)]
    impl MeasureCache {
        /// Last frame's measured `desired` column, which the layout
        /// tests read to prove what a capture retained.
        pub(crate) fn captured_desired(&self) -> &[Size] {
            &self.previous.nodes.desired
        }
    }
}

#[cfg(test)]
mod tests;
