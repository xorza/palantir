//! Cross-frame measure cache with one whole-tree snapshot per frame.
//! Measure reads only from the previous snapshot while the completed
//! current layout is materialized once in pre-order. Each node, grid
//! hug value, and shaped text run is therefore retained exactly once.

#[cfg(feature = "bench")]
pub(crate) mod bench;

use crate::common::content_hash::ContentHash;
use crate::common::counters::BenchOnly;
use crate::layout::ShapedText;
use crate::layout::grid::grid_track_store::GridTrackStore;
use crate::layout::intrinsic::SLOT_COUNT;
use crate::layout::types::layout_mode::LayoutMode;
use crate::primitives::num::F32Px;
use crate::primitives::rect::Rect;
use crate::primitives::size::Size;
use crate::primitives::span::Span;
use crate::primitives::widget_id::{WidgetId, WidgetIdMap};
use crate::scene::forest::Forest;
use crate::scene::layer::Layer;
use crate::scene::tree::Tree;
use glam::IVec2;

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
    pub(super) root: Size,
    /// Arena index this subtree's columns start at. `arrange` stores it
    /// per node so it can re-slice `rect` without a second map probe.
    pub(super) nodes_base: u32,
    pub(super) desired: &'a [Size],
    pub(super) scroll_content: &'a [Size],
    pub(super) text_spans: &'a [Span],
    pub(super) intrinsics: &'a [[f32; SLOT_COUNT]],
    pub(super) available_q: &'a [AvailableKey],
    pub(super) tracks: &'a [f32],
    pub(super) text_shapes: &'a [ShapedText],
    pub(super) text_shapes_base: u32,
}

#[derive(Debug)]
pub(super) struct CaptureTreeInput<'a> {
    pub(super) desired: &'a mut Vec<Size>,
    pub(super) rect: &'a [Rect],
    pub(super) scroll_content: &'a [Size],
    pub(super) intrinsics: &'a [[f32; SLOT_COUNT]],
    pub(super) available_q: &'a mut Vec<AvailableKey>,
    pub(super) grid_track_state: &'a GridTrackStore,
    pub(super) text_spans: &'a [Span],
    pub(super) text_shapes: &'a [ShapedText],
}

/// `pub(crate)` for one caller outside `layout`:
/// `text::wrap::tests::wrap_target_matches_cache_grid`, which pins the
/// wrap width against this very grid.
#[inline]
pub(crate) fn quantize_available(s: Size) -> AvailableKey {
    debug_assert!(s.w >= 0.0 && s.h >= 0.0, "negative available: {s:?}");
    IVec2::new(s.w.quantize_px(), s.h.quantize_px())
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
    /// Arranged rect per node, captured after `arrange` wrote it. The
    /// only column produced by the *second* half of the layout pass;
    /// `LayoutEngine::arrange` replays it instead of re-running the
    /// drivers when a subtree's slot is unchanged or merely translated.
    rect: Vec<Rect>,
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
            rect,
            scroll_content,
            text_spans,
            intrinsics,
            available_q,
        } = self;
        desired.clear();
        rect.clear();
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
    /// widget's descriptor. That is survivable — the `subtree_hash` and
    /// `available_q` checks there reject a mismatched descriptor, so the
    /// worst case is a missed hit — but it would be silent, so
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
    /// [`CascadeCounters`](crate::scene::cascade::counters::CascadeCounters)
    /// states.
    pub(crate) snapshot_rebuilds: BenchOnly<u32>,
}

impl MeasureCache {
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
                    subtree_hash: tree.rollups.subtree[root.idx()],
                    available_q: quantize_available(slot.available(layer, surface)),
                };
                if snapshot.roots[root_index] != current {
                    return false;
                }
                root_index += 1;
            }
        }
        true
    }

    /// Last frame's arranged rects for a subtree of `len` nodes whose
    /// capture starts at `base` — the third snapshot read, beside
    /// [`Self::try_lookup`] and [`Self::lookup_root_intrinsic`], rather
    /// than an index into the arena from outside.
    pub(super) fn arranged_rects(&self, base: usize, len: usize) -> &[Rect] {
        &self.previous.nodes.rect[base..base + len]
    }

    #[inline]
    pub(super) fn try_lookup(
        &self,
        wid: WidgetId,
        curr_hash: ContentHash,
        curr_avail: AvailableKey,
    ) -> Option<CachedSubtree<'_>> {
        let descriptor = *self.previous.snapshots.get(&wid)? as usize;
        let snap = &self.previous.descriptors[descriptor];
        if snap.subtree_hash != curr_hash || snap.available_q != curr_avail {
            return None;
        }
        let nodes = snap.nodes.range();
        Some(CachedSubtree {
            root: self.previous.nodes.desired[nodes.start],
            nodes_base: snap.nodes.start,
            desired: &self.previous.nodes.desired[nodes.clone()],
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
            rect,
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
        debug_assert_eq!(rect.len(), node_count);
        debug_assert_eq!(scroll_content.len(), node_count);
        debug_assert_eq!(intrinsics.len(), node_count);
        debug_assert_eq!(available_q.len(), node_count);
        debug_assert_eq!(text_spans.len(), node_count);

        let node_base = self.current.nodes.desired.len() as u32;
        let text_base = self.current.text_shapes.len() as u32;

        self.current.nodes.rect.extend_from_slice(rect);
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
                subtree_hash: tree.rollups.subtree[index],
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
                subtree_hash: tree.rollups.subtree[index],
                available_q: available_q[index],
                nodes: Span::new(node_base + index as u32, (end - index) as u32),
                tracks,
                text_shapes,
            });
            self.current.descriptor_wids.push(wid);
        }

        if node_base == 0 {
            std::mem::swap(&mut self.current.nodes.desired, desired);
            std::mem::swap(&mut self.current.nodes.available_q, available_q);
        } else {
            self.current.nodes.desired.extend_from_slice(desired);
            self.current
                .nodes
                .available_q
                .extend_from_slice(available_q);
        }
    }

    pub(super) fn end_frame(&mut self) {
        if self.current.refresh_snapshots() {
            self.snapshot_rebuilds.bump();
        }
        std::mem::swap(&mut self.previous, &mut self.current);
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

#[cfg(test)]
pub(crate) mod test_support {
    use super::*;

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
