//! Cross-frame measure cache with one whole-tree snapshot per frame.
//! Measure reads only from the previous snapshot while the completed
//! current layout is materialized once in pre-order. Each node, grid
//! hug value, and shaped text run is therefore retained exactly once.

use crate::common::content_hash::ContentHash;
use crate::layout::ShapedText;
use crate::layout::grid::GridHugStore;
use crate::layout::intrinsic::SLOT_COUNT;
use crate::layout::types::layout_mode::LayoutMode;
use crate::primitives::size::Size;
use crate::primitives::span::Span;
use crate::primitives::widget_id::{WidgetId, WidgetIdMap};
use crate::scene::tree::Tree;
use glam::IVec2;

#[derive(Clone, Copy, Debug)]
pub(crate) struct ArenaSnapshot {
    pub(crate) subtree_hash: ContentHash,
    pub(crate) available_q: AvailableKey,
    pub(crate) nodes: Span,
    pub(crate) hugs: Span,
    pub(crate) text_shapes: Span,
}

pub(crate) type AvailableKey = IVec2;

pub(crate) const INVALID_AVAILABLE: AvailableKey = IVec2::splat(i32::MIN);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct RootSnapshotKey {
    pub(crate) wid: WidgetId,
    pub(crate) subtree_hash: ContentHash,
    pub(crate) available_q: AvailableKey,
}

#[derive(Debug)]
pub(crate) struct CachedSubtree<'a> {
    pub(crate) root: Size,
    pub(crate) desired: &'a [Size],
    pub(crate) text_spans: &'a [Span],
    pub(crate) intrinsics: &'a [[f32; SLOT_COUNT]],
    pub(crate) available_q: &'a [AvailableKey],
    pub(crate) hugs: &'a [f32],
    pub(crate) text_shapes: &'a [ShapedText],
    pub(crate) text_shapes_base: u32,
}

#[derive(Debug)]
pub(crate) struct CaptureTreeInput<'a> {
    pub(crate) desired: &'a mut Vec<Size>,
    pub(crate) intrinsics: &'a [[f32; SLOT_COUNT]],
    pub(crate) available_q: &'a mut Vec<AvailableKey>,
    pub(crate) grid_hugs: &'a GridHugStore,
    pub(crate) text_spans: &'a [Span],
    pub(crate) text_shapes: &'a [ShapedText],
}

#[inline]
fn quantize_axis(v: f32) -> i32 {
    if !v.is_finite() {
        i32::MAX
    } else {
        v.round() as i32
    }
}

#[inline]
pub(crate) fn quantize_available(s: Size) -> AvailableKey {
    assert!(s.w >= 0.0 && s.h >= 0.0, "negative available: {s:?}");
    IVec2::new(quantize_axis(s.w), quantize_axis(s.h))
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
    pub(crate) desired: Vec<Size>,
    pub(crate) text_spans: Vec<Span>,
    pub(crate) intrinsics: Vec<[f32; SLOT_COUNT]>,
    pub(crate) available_q: Vec<AvailableKey>,
}

impl NodeArenas {
    fn clear(&mut self) {
        self.desired.clear();
        self.text_spans.clear();
        self.intrinsics.clear();
        self.available_q.clear();
    }
}

#[derive(Debug, Default)]
pub(crate) struct MeasureSnapshot {
    pub(crate) nodes: NodeArenas,
    pub(crate) hugs: Vec<f32>,
    pub(crate) text_shapes: Vec<ShapedText>,
    pub(crate) snapshots: WidgetIdMap<u32>,
    pub(crate) descriptors: Vec<ArenaSnapshot>,
    descriptor_wids: Vec<WidgetId>,
    pub(crate) roots: Vec<RootSnapshotKey>,
    descriptor_identity: u64,
}

impl MeasureSnapshot {
    fn begin_capture(&mut self) {
        self.nodes.clear();
        self.hugs.clear();
        self.text_shapes.clear();
        self.descriptors.clear();
        self.descriptor_wids.clear();
        self.roots.clear();
        self.descriptor_identity = 0;
    }

    #[cfg(any(test, feature = "internals"))]
    fn clear(&mut self) {
        self.begin_capture();
        self.snapshots.clear();
    }
}

#[derive(Debug, Default)]
pub(crate) struct MeasureCache {
    pub(crate) previous: MeasureSnapshot,
    current: MeasureSnapshot,
    hug_offsets: Vec<u32>,
    text_bounds: Vec<Span>,
    previous_descriptor_identity: u64,
}

impl MeasureCache {
    pub(crate) fn begin_frame(&mut self) {
        self.previous_descriptor_identity = self.current.descriptor_identity;
        self.current.begin_capture();
    }

    #[inline]
    pub(crate) fn try_lookup(
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
            desired: &self.previous.nodes.desired[nodes.clone()],
            text_spans: &self.previous.nodes.text_spans[nodes.clone()],
            intrinsics: &self.previous.nodes.intrinsics[nodes.clone()],
            available_q: &self.previous.nodes.available_q[nodes],
            hugs: &self.previous.hugs[snap.hugs.range()],
            text_shapes: &self.previous.text_shapes[snap.text_shapes.range()],
            text_shapes_base: snap.text_shapes.start,
        })
    }

    #[inline]
    pub(crate) fn lookup_root_intrinsic(
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

    pub(crate) fn capture_tree(&mut self, tree: &Tree, input: CaptureTreeInput<'_>) {
        let CaptureTreeInput {
            desired,
            intrinsics,
            available_q,
            grid_hugs,
            text_spans,
            text_shapes,
        } = input;
        let node_count = tree.records.len();
        assert_eq!(desired.len(), node_count);
        assert_eq!(intrinsics.len(), node_count);
        assert_eq!(available_q.len(), node_count);
        assert_eq!(text_spans.len(), node_count);

        let node_base = self.current.nodes.desired.len() as u32;
        let text_base = self.current.text_shapes.len() as u32;

        self.current.nodes.intrinsics.extend_from_slice(intrinsics);
        let has_text = !text_shapes.is_empty();
        if has_text {
            self.current.text_shapes.extend_from_slice(text_shapes);
            self.text_bounds.clear();
            self.text_bounds.resize(node_count, Span::default());
            let mut owned_text_count = 0u32;
            for (index, span) in text_spans.iter().copied().enumerate() {
                if span.len != 0 {
                    let stored = Span::new(text_base + span.start, span.len);
                    self.current.nodes.text_spans.push(stored);
                    self.text_bounds[index] = stored;
                    owned_text_count += span.len;
                } else {
                    self.current.nodes.text_spans.push(Span::default());
                }
            }
            assert_eq!(owned_text_count as usize, text_shapes.len());

            for index in (0..node_count).rev() {
                let end = tree.subtree_end_of(index) as usize;
                let mut bound = self.text_bounds[index];
                let mut run_count = bound.len;
                let mut child = index + 1;
                while child < end {
                    let child_bound = self.text_bounds[child];
                    run_count += child_bound.len;
                    bound = union_spans(bound, child_bound);
                    child = tree.subtree_end_of(child) as usize;
                }
                assert_eq!(
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
            self.hug_offsets.clear();
            self.hug_offsets.resize(node_count + 1, 0);
            for (index, style) in layouts.iter().copied().enumerate() {
                self.hug_offsets[index] = self.current.hugs.len() as u32;
                if matches!(LayoutMode::from(style.meta), LayoutMode::Grid(_)) {
                    grid_hugs.snapshot_subtree(tree, index..index + 1, &mut self.current.hugs);
                }
            }
            self.hug_offsets[node_count] = self.current.hugs.len() as u32;
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
            let end = tree.subtree_end_of(index) as usize;
            let hugs = if has_grids {
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
                hugs,
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

    pub(crate) fn finish_frame(&mut self) {
        if self.current.descriptor_identity != self.previous_descriptor_identity
            || self.current.snapshots.len() != self.current.descriptors.len()
        {
            self.current.snapshots.clear();
            for (descriptor, wid) in self.current.descriptor_wids.iter().copied().enumerate() {
                self.current.snapshots.insert(wid, descriptor as u32);
            }
        }
        std::mem::swap(&mut self.previous, &mut self.current);
    }

    #[cfg(any(test, feature = "internals"))]
    pub(crate) fn clear(&mut self) {
        self.previous.clear();
        self.current.clear();
    }
}

#[cfg(test)]
mod integration_tests;
#[cfg(test)]
mod tests;
