//! Cross-frame measure cache with one whole-tree snapshot per frame. Measure
//! reads only the previous snapshot; the completed layout is materialized once
//! in pre-order, so each node, grid hug value and shaped text run is retained
//! once.
//!
//! The snapshot is a copy of the engine's columns, taken on a frame that
//! rebuilds it. Refused: measure and arrange writing the snapshot columns in
//! place, which moves every reader of `Layout` onto forest-wide columns to save
//! the copy, about 1 % of a resizing frame (2026-10-28, `sample` on
//! `integration/frame/cpu/resizing`).

#[cfg(feature = "internals")]
pub(crate) mod bench;
pub(crate) mod measure_snapshot;
mod node_arenas;

use crate::common::content_hash::ContentHash;
use crate::common::counters::BenchOnly;
use crate::common::span::Span;
use crate::layout::cache::measure_snapshot::{ArenaSnapshot, MeasureSnapshot, SubtreeKey};
use crate::layout::drivers::grid::grid_track_store::GridTrackStore;
use crate::layout::measure_columns::MeasureColumns;
use crate::layout::measured::Measured;
use crate::layout::plane_layout::PlaneLayout;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::geometry::size::Size;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::layout::layout_mode::LayoutMode;
use crate::scene::tree::Tree;
use crate::text::shaped_text::ShapedText;
use glam::Vec2;
use std::mem;
use std::ops::Range;

/// The `available` of a node no measure reached this frame; no subtree is captured under it.
pub(super) const UNMEASURED: Size = Size::new(f32::NAN, f32::NAN);

/// Whether two offers are the same one, bit for bit; `UNMEASURED` matches itself. A kept run
/// needs the exact offer, where [`MeasureCache::try_lookup`] also serves any finite offer past
/// `stable_from`.
pub(super) const fn same_offer(a: Size, b: Size) -> bool {
    a.w.to_bits() == b.w.to_bits() && a.h.to_bits() == b.h.to_bits()
}

/// Last frame's measure of one subtree, as a hit restores it: the arena rows it takes, each
/// column of which holds the subtree's nodes in pre-order.
#[derive(Debug)]
pub(super) struct CachedSubtree<'a> {
    pub(super) root: Measured,
    pub(super) rows: Range<usize>,
    pub(super) measure: &'a MeasureColumns,
    pub(super) scroll_content: &'a [Size],
    pub(super) text_spans: &'a [Span],
    pub(super) tracks: &'a [f32],
    pub(super) text_shapes: &'a [ShapedText],
    pub(super) text_shapes_base: u32,
}

/// A container's paint-only runs as last frame shaped them, and the width it was arranged to.
#[derive(Clone, Copy, Debug)]
pub(super) struct CachedContainerText<'a> {
    pub(super) width: f32,
    pub(super) shapes: &'a [ShapedText],
}

/// A subtree's arrange as last frame captured it: each node's rect and its
/// slot origin in its parent's inner box.
#[derive(Debug)]
pub(super) struct Arranged<'a> {
    pub(super) rects: &'a [Rect],
    pub(super) locals: &'a [Vec2],
}

/// The engine's columns for one laid-out tree, as [`MeasureCache::capture_tree`] appends them:
/// what measure left, the plane's output, each node's slot origin, and the grids' tracks.
#[derive(Clone, Copy, Debug)]
pub(super) struct CaptureTreeInput<'a> {
    pub(super) measure: &'a MeasureColumns,
    pub(super) layout: &'a PlaneLayout,
    pub(super) local: &'a [Vec2],
    pub(super) grid_track_state: &'a GridTrackStore,
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
    pub(super) fn begin_frame(&mut self) {
        self.cur.clear_capture();
    }

    /// Whether last frame's capture holds a node for each of `nodes`, the forest's: a capture
    /// that a font load or no frame emptied holds none. The roots' keys, which the engine keeps
    /// with each run, say the rest.
    pub(super) fn holds_nodes(&self, nodes: usize) -> bool {
        self.prev.nodes.measure.len() == nodes
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
    /// unchanged and it holds under `key.available`: per axis, the offer it was
    /// measured at, bit for bit, or any finite offer past [`Measured::stable_from`].
    #[inline]
    pub(super) fn try_lookup(&self, key: SubtreeKey) -> Option<CachedSubtree<'_>> {
        let snap = self.prev.desc(key.widget_id)?;
        if snap.subtree_hash != key.subtree_hash {
            return None;
        }
        let available = key.available;
        let rows = snap.nodes.range();
        let measure = &self.prev.nodes.measure;
        let stable_from = measure.stable_from[rows.start];
        let holds = |measured_at: f32, offer: f32, from: f32| {
            measured_at.to_bits() == offer.to_bits() || (offer.is_finite() && offer >= from)
        };
        if !(holds(snap.available.w, available.w, stable_from.w)
            && holds(snap.available.h, available.h, stable_from.h))
        {
            return None;
        }
        Some(CachedSubtree {
            root: Measured {
                size: measure.desired[rows.start],
                floor: measure.floor[rows.start],
                stable_from,
            },
            measure,
            scroll_content: &self.prev.nodes.scroll_content[rows.clone()],
            text_spans: &self.prev.nodes.text_spans[rows.clone()],
            rows,
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
        let value = self.prev.nodes.measure.intrinsics[snap.nodes.start as usize][slot];
        (!value.is_nan()).then_some(value)
    }

    pub(super) fn capture_tree(&mut self, tree: &Tree, input: CaptureTreeInput<'_>) {
        let CaptureTreeInput {
            measure,
            layout,
            local,
            grid_track_state,
        } = input;
        let PlaneLayout {
            rect,
            scroll_content,
            text_shapes,
            text_spans,
            ..
        } = layout;
        let available = &measure.available;
        let node_count = tree.records.len();
        // Debug only: column-length agreement is an engine invariant, and this
        // runs once per plane per frame.
        debug_assert_eq!(measure.len(), node_count);
        debug_assert_eq!(rect.len(), node_count);
        debug_assert_eq!(local.len(), node_count);
        debug_assert_eq!(scroll_content.len(), node_count);
        debug_assert_eq!(text_spans.len(), node_count);

        let node_base = self.cur.nodes.measure.len() as u32;
        let text_base = self.cur.text_shapes.len() as u32;

        self.cur.nodes.rect.extend_from_slice(rect);
        self.cur.nodes.local.extend_from_slice(local);
        // Filled by `capture_container_text` once the container pass has shaped them.
        let containers_len = self.cur.nodes.container_spans.len() + node_count;
        self.cur.nodes.container_spans.resize(containers_len, None);
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

        for idx in 0..node_count {
            if LayoutMode::from(layouts[idx].meta) == LayoutMode::Leaf
                || same_offer(available[idx], UNMEASURED)
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
            self.cur.descs.push(ArenaSnapshot {
                subtree_hash: tree.rollups.layout_subtree[idx],
                available: available[idx],
                nodes: Span::new(node_base + idx as u32, (end - idx) as u32),
                tracks,
                text_shapes,
            });
            self.cur.desc_widget_ids.push(widget_id);
        }

        // Copied for every plane, the first included: a swap for the first
        // would leave the engine's scratch columns empty while the
        // container-text pass still runs.
        self.cur.nodes.measure.extend_from(measure);
    }

    /// Append the container runs `layout` holds for `tree`, the tree [`Self::capture_tree`]
    /// captured last, once the container pass has shaped them.
    pub(super) fn capture_container_text(&mut self, tree: &Tree, layout: &PlaneLayout) {
        let node_base = self.cur.nodes.measure.len() - tree.records.len();
        for idx in tree.container_text.ones() {
            let span = layout.text_spans[idx];
            let start = self.cur.text_shapes.len() as u32;
            self.cur
                .text_shapes
                .extend_from_slice(&layout.text_shapes[span.range()]);
            self.cur.nodes.container_spans[node_base + idx] = Some(Span::new(start, span.len));
        }
    }

    /// Last frame's container runs of the node at arena row `row`, with the width it was
    /// arranged to, which they were shaped against; `None` where last frame shaped none.
    pub(super) fn container_text(&self, row: usize) -> Option<CachedContainerText<'_>> {
        let nodes = &self.prev.nodes;
        let span = nodes.container_spans[row]?;
        Some(CachedContainerText {
            width: nodes.rect[row].size.w,
            shapes: &self.prev.text_shapes[span.range()],
        })
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

/// What the measure-cache tests reach.
#[cfg(test)]
pub(crate) mod internals {
    use crate::layout::cache::MeasureCache;
    use crate::primitives::geometry::size::Size;

    impl MeasureCache {
        /// Last frame's measured `desired` column.
        pub(crate) fn captured_desired(&self) -> &[Size] {
            &self.prev.nodes.measure.desired
        }
    }
}

#[cfg(test)]
mod tests;
