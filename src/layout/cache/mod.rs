//! Cross-frame measure cache with one whole-tree snapshot per frame. Measure
//! reads only the previous snapshot; the completed layout is materialized once
//! in pre-order, so each node, grid hug value and shaped text run is retained
//! once.

#[cfg(feature = "internals")]
pub(crate) mod bench;
pub(crate) mod measure_snapshot;
mod node_arenas;

use crate::common::content_hash::ContentHash;
use crate::common::counters::BenchOnly;
use crate::common::span::Span;
use crate::layout::cache::measure_snapshot::{ArenaSnapshot, MeasureSnapshot, SubtreeKey};
use crate::layout::drivers::grid::grid_track_store::GridTrackStore;
use crate::layout::intrinsic::len_req::SLOT_COUNT;
use crate::layout::measured::Measured;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::geometry::size::Size;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::layout::layout_mode::LayoutMode;
use crate::primitives::math::num::F32Px;
use crate::scene::forest::Forest;
use crate::scene::tree::Tree;
use crate::text::shaped_text::ShapedText;
use glam::{IVec2, Vec2};
use std::mem;

/// An offer quantized to the grid [`MeasureCache::try_lookup`] matches on.
pub(super) type AvailableKey = IVec2;

/// The `available_q` of a node no measure reached this frame; no subtree is captured under it.
pub(super) const INVALID_AVAILABLE: AvailableKey = IVec2::splat(i32::MIN);

/// Last frame's measure of one subtree, as a hit restores it.
#[derive(Debug)]
pub(super) struct CachedSubtree<'a> {
    pub(super) root: Measured,
    /// Arena index this subtree's columns start at.
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

/// A subtree's arrange as last frame captured it: each node's rect and its
/// slot origin in its parent's inner box.
#[derive(Debug)]
pub(super) struct Arranged<'a> {
    pub(super) rects: &'a [Rect],
    pub(super) locals: &'a [Vec2],
}

/// The engine's columns for one laid-out tree, as [`MeasureCache::capture_tree`] appends them.
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
pub(crate) struct MeasureCache {
    prev: MeasureSnapshot,
    cur: MeasureSnapshot,
    hug_offsets: Vec<u32>,
    text_bounds: Vec<Span>,
    /// Snapshot-map rebuilds so far, letting a test prove the reuse gate in
    /// [`Self::end_frame`] fires and busts. Accumulates, so readers take a
    /// delta, as with [`CascadeCounters`](crate::cascade::counters::CascadeCounters).
    pub(crate) snapshot_rebuilds: BenchOnly<u32>,
}

impl MeasureCache {
    /// The cache key for an available size: each axis quantized to the grid
    /// `try_lookup` matches on. `pub(crate)` for
    /// `text::wrap::tests::wrap_target_matches_cache_grid`.
    #[inline]
    pub(crate) fn available_key(s: Size) -> AvailableKey {
        debug_assert!(s.w >= 0.0 && s.h >= 0.0, "negative available: {s:?}");
        IVec2::new(s.w.quantize_px(), s.h.quantize_px())
    }

    pub(super) fn begin_frame(&mut self) {
        self.cur.clear_capture();
    }

    /// Whether last frame's capture still describes `forest` at `surface`:
    /// same node and root counts, and every root under the same widget id,
    /// subtree hash and quantized available. `false` forces a rebuild.
    pub(super) fn matches_forest(&self, forest: &Forest, surface: Rect) -> bool {
        let snapshot = &self.prev;
        if snapshot.nodes.desired.len() != forest.total_nodes()
            || snapshot.roots.len() != forest.total_roots()
        {
            return false;
        }
        let mut root_idx = 0;
        for tree in forest.trees.iter_paint_order().map(|(_, tree)| tree) {
            for slot in &tree.roots {
                let root = slot.first_node;
                let cur = SubtreeKey {
                    widget_id: tree.records.widget_id()[root.idx()],
                    subtree_hash: tree.rollups.layout_subtree[root.idx()],
                    available_q: Self::available_key(slot.available(surface)),
                };
                if snapshot.roots[root_idx] != cur {
                    return false;
                }
                root_idx += 1;
            }
        }
        true
    }

    /// Last frame's arrange of a subtree of `len` nodes whose capture starts
    /// at `base`.
    pub(super) fn arranged(&self, base: usize, len: usize) -> Arranged<'_> {
        let nodes = base..base + len;
        Arranged {
            rects: &self.prev.nodes.rect[nodes.clone()],
            locals: &self.prev.nodes.local[nodes],
        }
    }

    /// Last frame's measure of the subtree `key` names, if its authoring is
    /// unchanged and it holds under `available` (which `key.available_q`
    /// quantizes): the offer it was measured at (to the key's grid), or any
    /// finite offer past [`Measured::stable_from`], per axis.
    #[inline]
    pub(super) fn try_lookup(&self, key: SubtreeKey, available: Size) -> Option<CachedSubtree<'_>> {
        let snap = self.prev.desc(key.widget_id)?;
        if snap.subtree_hash != key.subtree_hash {
            return None;
        }
        let available_q = key.available_q;
        let nodes = snap.nodes.range();
        let stable_from = self.prev.nodes.stable_from[nodes.start];
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
                size: self.prev.nodes.desired[nodes.start],
                floor: self.prev.nodes.floor[nodes.start],
                stable_from,
            },
            nodes_base: snap.nodes.start,
            desired: &self.prev.nodes.desired[nodes.clone()],
            floor: &self.prev.nodes.floor[nodes.clone()],
            stable_from: &self.prev.nodes.stable_from[nodes.clone()],
            scroll_content: &self.prev.nodes.scroll_content[nodes.clone()],
            text_spans: &self.prev.nodes.text_spans[nodes.clone()],
            intrinsics: &self.prev.nodes.intrinsics[nodes.clone()],
            available_q: &self.prev.nodes.available_q[nodes],
            tracks: &self.prev.tracks[snap.tracks.range()],
            text_shapes: &self.prev.text_shapes[snap.text_shapes.range()],
            text_shapes_base: snap.text_shapes.start,
        })
    }

    /// Last frame's intrinsic `slot` of `widget_id`'s subtree root, if its authoring
    /// is unchanged and the slot was computed.
    #[inline]
    pub(super) fn lookup_root_intrinsic(
        &self,
        widget_id: WidgetId,
        subtree_hash: ContentHash,
        slot: usize,
    ) -> Option<f32> {
        let snap = self.prev.desc(widget_id)?;
        if snap.subtree_hash != subtree_hash {
            return None;
        }
        let value = self.prev.nodes.intrinsics[snap.nodes.start as usize][slot];
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
        // Debug only: column-length agreement is an engine invariant, and this
        // runs once per plane per frame.
        debug_assert_eq!(desired.len(), node_count);
        debug_assert_eq!(floor.len(), node_count);
        debug_assert_eq!(stable_from.len(), node_count);
        debug_assert_eq!(rect.len(), node_count);
        debug_assert_eq!(local.len(), node_count);
        debug_assert_eq!(scroll_content.len(), node_count);
        debug_assert_eq!(intrinsics.len(), node_count);
        debug_assert_eq!(available_q.len(), node_count);
        debug_assert_eq!(text_spans.len(), node_count);

        let node_base = self.cur.nodes.desired.len() as u32;
        let text_base = self.cur.text_shapes.len() as u32;

        self.cur.nodes.rect.extend_from_slice(rect);
        self.cur.nodes.local.extend_from_slice(local);
        self.cur.nodes.intrinsics.extend_from_slice(intrinsics);
        self.cur
            .nodes
            .scroll_content
            .extend_from_slice(scroll_content);
        let has_text = !text_shapes.is_empty();
        if has_text {
            self.cur.text_shapes.extend_from_slice(text_shapes);
            // Bare `resize`: the loop below writes every slot, and `clear`
            // would add a truncate-then-regrow round trip.
            self.text_bounds.resize(node_count, Span::default());
            let mut owned_text_count = 0u32;
            for (idx, span) in text_spans.iter().copied().enumerate() {
                // Both arms assign so the column is total; a text-less node
                // must not inherit a longer previous tree's bound.
                let stored = if span.len == 0 {
                    Span::default()
                } else {
                    owned_text_count += span.len;
                    Span::new(text_base + span.start, span.len)
                };
                self.cur.nodes.text_spans.push(stored);
                self.text_bounds[idx] = stored;
            }
            debug_assert_eq!(owned_text_count as usize, text_shapes.len());

            for idx in (0..node_count).rev() {
                let end = tree.subtree_end_of(idx);
                let mut bound = self.text_bounds[idx];
                let mut run_count = bound.len;
                let mut child = idx + 1;
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
                self.text_bounds[idx] = bound;
            }
        } else {
            self.cur.nodes.text_spans.resize(
                self.cur.nodes.text_spans.len() + node_count,
                Span::default(),
            );
        }

        let layouts = tree.records.layout();
        let has_grids = !tree.grid_defs.is_empty();
        if has_grids {
            // Every slot `0..=node_count` is assigned below.
            self.hug_offsets.resize(node_count + 1, 0);
            for (idx, layout) in layouts.iter().copied().enumerate() {
                self.hug_offsets[idx] = self.cur.tracks.len() as u32;
                if let LayoutMode::Grid(grid) = LayoutMode::from(layout.meta) {
                    grid_track_state.snapshot_grid(grid, &mut self.cur.tracks);
                }
            }
            self.hug_offsets[node_count] = self.cur.tracks.len() as u32;
        }

        for slot in &tree.roots {
            let idx = slot.first_node.idx();
            self.cur.roots.push(SubtreeKey {
                widget_id: tree.records.widget_id()[idx],
                subtree_hash: tree.rollups.layout_subtree[idx],
                available_q: available_q[idx],
            });
        }

        for idx in 0..node_count {
            if LayoutMode::from(layouts[idx].meta) == LayoutMode::Leaf
                || available_q[idx] == INVALID_AVAILABLE
            {
                continue;
            }
            let end = tree.subtree_end_of(idx);
            let tracks = if has_grids {
                let start = self.hug_offsets[idx];
                Span::new(start, self.hug_offsets[end] - start)
            } else {
                Span::default()
            };
            let text_shapes = if has_text {
                self.text_bounds[idx]
            } else {
                Span::default()
            };
            let widget_id = tree.records.widget_id()[idx];
            self.cur.desc_identity = (self.cur.desc_identity.rotate_left(5) ^ widget_id.0)
                .wrapping_mul(0x517c_c1b7_2722_0a95);
            self.cur.descs.push(ArenaSnapshot {
                subtree_hash: tree.rollups.layout_subtree[idx],
                available_q: available_q[idx],
                nodes: Span::new(node_base + idx as u32, (end - idx) as u32),
                tracks,
                text_shapes,
            });
            self.cur.desc_widget_ids.push(widget_id);
        }

        // Copied for every plane, the first included: a swap for the first
        // would leave the engine's scratch columns empty while the
        // container-text pass still runs.
        self.cur.nodes.desired.extend_from_slice(desired);
        self.cur.nodes.floor.extend_from_slice(floor);
        self.cur.nodes.stable_from.extend_from_slice(stable_from);
        self.cur.nodes.available_q.extend_from_slice(available_q);
    }

    pub(super) fn end_frame(&mut self) {
        if self.cur.refresh_snapshots() {
            self.snapshot_rebuilds.bump();
        }
        mem::swap(&mut self.prev, &mut self.cur);
    }

    /// Force a cold start: both buffers forget everything. The other
    /// invalidations ask whether inputs moved, which a font load does not
    /// change (see `TextSystem::sync_fonts`).
    pub(super) fn forget_all(&mut self) {
        self.prev.forget_all();
        self.cur.forget_all();
    }
}

/// What the measure-cache tests reach, and the tree shapes the bench times:
/// a deep chain and a balanced broad tree.
#[cfg(feature = "internals")]
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

    /// What a variant of the broad tree changes in its first leaf. Both are
    /// layout authoring; a color is paint, which the cache does not key on.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub(crate) enum BroadChange {
        /// The leaf's fill weight, which an only child's geometry ignores.
        FillWeight,
        /// The leaf's height, which moves the minimum of every panel above it.
        LeafHeight,
    }

    pub(crate) fn build_broad(ui: &mut Ui) {
        build_broad_variant(ui, None);
    }

    pub(crate) fn build_broad_variant(ui: &mut Ui, change: Option<BroadChange>) {
        BroadLevel {
            depth: 0,
            key: 0,
            change,
        }
        .show(ui);
    }

    /// One panel of the broad tree: `depth` levels below the root, `key` its
    /// pre-order index among its level's panels.
    #[derive(Clone, Copy, Debug)]
    struct BroadLevel {
        depth: usize,
        key: usize,
        change: Option<BroadChange>,
    }

    impl BroadLevel {
        fn show(self, ui: &mut Ui) {
            let Self { depth, key, change } = self;
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
                        Self {
                            depth: depth + 1,
                            key: key * BROAD_FANOUT + child,
                            change,
                        }
                        .show(ui);
                    }
                });
        }
    }

    #[cfg(test)]
    impl MeasureCache {
        /// Last frame's measured `desired` column.
        pub(crate) fn captured_desired(&self) -> &[Size] {
            &self.prev.nodes.desired
        }
    }
}

#[cfg(test)]
mod tests;
