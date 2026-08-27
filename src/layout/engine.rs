//! The layout walk: [`LayoutEngine`], the scratch it carries between frames,
//! and the snapshot check that decides whether last frame's measurements
//! still describe this frame's forest.

use crate::common::tracy;
use crate::layout::axis::Axis;
use crate::layout::axis_placement::AxisPlacement;
use crate::layout::cache::{
    CaptureTreeInput, MeasureCache, MeasureSnapshot, RootSnapshotKey, quantize_available,
};
use crate::layout::counters::PhaseSpan;
use crate::layout::intrinsic::{IntrinsicQuery, IntrinsicRange, LenReq};
use crate::layout::layout_scratch::LayoutScratch;
use crate::layout::pass::LayoutPass;
use crate::layout::text_shape_input::TextShapeInput;
use crate::layout::types::layout_mode::LayoutMode;
use crate::layout::{Layout, intrinsic};
use crate::primitives::interned_text::InternedText;
use crate::primitives::rect::Rect;
use crate::primitives::size::Size;
use crate::scene::forest::Forest;
use crate::scene::layer::Layer;
use crate::scene::tree::Tree;
use crate::scene::tree::node_id::NodeId;
use crate::scene::tree::root_slot::RootSlot;
use crate::text::shaper::TextShaper;
use crate::text::system::TextSystem;

/// Size offered to one layer root.
///
/// `Layer::Main` fills the surface; every overlay layer derives its own from
/// its [`Placement`](crate::layout::types::placement::Placement). Shared by
/// [`LayoutEngine::run`], which measures against
/// it, and [`LayoutEngine::cache_snapshot_matches_forest`], which quantizes it
/// into the snapshot's root key — those two **must** agree, or the key
/// describes an offer measure never saw and every root misses.
fn root_available(layer: Layer, slot: &RootSlot, surface: Rect) -> Size {
    if layer == Layer::Main {
        surface.size
    } else {
        slot.placement.available(surface)
    }
}

/// Persistent layout engine. Field groups by lifetime:
///
/// - `scratch` — per-frame intermediate state (see [`LayoutScratch`]).
///   Cleared at the top of every `run`.
/// - `text` — per-window text shaping and reuse slots.
/// - `cache` — cross-frame measure cache. See [`crate::layout::cache`].
///
/// Per-frame *output* is **not** held here: `run` threads it through an
/// `out: &mut Layout`, so the finalized layout is owned by the caller
/// and read by the encoder, cascade, hit-index, scroll-state refresh,
/// and tests. Recursive work receives only the current [`LayerLayout`](crate::layout::LayerLayout)
/// slot.
#[derive(Debug)]
pub(crate) struct LayoutEngine {
    pub(crate) scratch: LayoutScratch,
    pub(crate) text: TextSystem,
    pub(crate) cache: MeasureCache,
}

impl LayoutEngine {
    pub(crate) fn new(shaper: TextShaper) -> Self {
        Self {
            scratch: LayoutScratch::default(),
            text: TextSystem::new(shaper),
            cache: MeasureCache::default(),
        }
    }

    fn cache_snapshot_matches_forest(
        snapshot: &MeasureSnapshot,
        forest: &Forest,
        surface: Rect,
    ) -> bool {
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
                let available = root_available(layer, slot, surface);
                let current = RootSnapshotKey {
                    wid: tree.records.widget_id()[root.idx()],
                    subtree_hash: tree.rollups.subtree[root.idx()],
                    available_q: quantize_available(available),
                };
                if snapshot.roots[root_index] != current {
                    return false;
                }
                root_index += 1;
            }
        }
        true
    }

    /// Grid's per-track intrinsic aggregator — a bump stack `grid::intrinsic`
    /// extends, recurses through, and truncates back. Reached by name for
    /// the same reason [`LayoutPass`]'s accessors exist; the intrinsic
    /// query itself stays off the pass, so it asks the engine directly.
    #[inline]
    pub(super) fn grid_track_aggregator(&mut self) -> &mut Vec<f32> {
        &mut self.scratch.grid.track_aggregator
    }

    /// Cross-frame intrinsic for one `(node, axis, req)` slot, or `None`
    /// when the node is ineligible or the snapshot has no value.
    ///
    /// Only non-leaf nodes are cacheable: a leaf's intrinsic is cheap to
    /// recompute and it owns no descriptor. Intrinsics are independent of
    /// the parent's `available`, so this checks `subtree_hash` alone and
    /// hits even on a resize frame where `try_lookup` misses on
    /// `available_q`.
    #[inline]
    fn cached_intrinsic(&self, tree: &Tree, idx: usize, slot: usize) -> Option<f32> {
        if LayoutMode::from(tree.records.layout()[idx].meta) == LayoutMode::Leaf {
            return None;
        }
        self.cache.lookup_root_intrinsic(
            tree.records.widget_id()[idx],
            tree.rollups.subtree[idx],
            slot,
        )
    }

    /// On-demand intrinsic-size query — outer (margin-inclusive) size on
    /// `axis` for the halves `query` asks for.
    ///
    /// Pure function of the subtree at `node`: independent of the
    /// parent's available width and of the arranged rect. Three layers
    /// answer it, cheapest first — the intra-frame slot array, last
    /// frame's snapshot, then a real subtree walk — and only the halves
    /// still missing after the first two reach the walk, so a range query
    /// whose min is already cached costs a max-only recursion.
    ///
    /// Consumed by `grid::measure` (Phase 1 column resolution) and
    /// `stack::measure` (Fill min-content floor) via the thin
    /// [`Self::intrinsic`] / [`Self::intrinsic_range`] wrappers.
    pub(super) fn intrinsic_query(
        &mut self,
        tree: &Tree,
        node: NodeId,
        axis: Axis,
        query: IntrinsicQuery,
        interned_text: &InternedText<'_>,
    ) -> IntrinsicRange {
        let idx = node.idx();
        let mut range = IntrinsicRange::ZERO;
        let (mut missing_min, mut missing_max) = (false, false);
        for (req, slot) in range.requested(query) {
            let cached = self.scratch.intrinsics[idx][req.slot(axis)];
            if !cached.is_nan() {
                *slot = cached;
                continue;
            }
            // Cross-frame reuse: an unchanged subtree's intrinsic is still
            // valid from last frame's measure-cache snapshot. Intrinsic is
            // `available`-independent, so this hits even on a resize frame
            // where the desired-cache (`try_lookup`) misses on
            // `available_q`. Crucially it fires at the *query* site: a
            // parent computes its `intrinsic_min` (which queries children)
            // before measuring those children, so the children's own
            // cache-hit restore comes too late — only a lookup here stops
            // the ancestor cold-recursing through every unchanged sibling
            // subtree.
            if let Some(value) = self.cached_intrinsic(tree, idx, req.slot(axis)) {
                self.scratch.intrinsics[idx][req.slot(axis)] = value;
                *slot = value;
                continue;
            }
            match req {
                LenReq::MinContent => missing_min = true,
                LenReq::MaxContent => missing_max = true,
            }
        }

        let Some(walk) = IntrinsicQuery::of(missing_min, missing_max) else {
            return range;
        };
        self.scratch.counters.intrinsic_computed();
        let computed = intrinsic::compute(self, tree, node, axis, walk, interned_text);
        for (req, slot) in range.requested(walk) {
            let value = computed.get(req);
            self.scratch.intrinsics[idx][req.slot(axis)] = value;
            *slot = value;
        }
        range
    }

    /// One half of [`Self::intrinsic_query`].
    #[inline]
    pub(super) fn intrinsic(
        &mut self,
        tree: &Tree,
        node: NodeId,
        axis: Axis,
        req: LenReq,
        interned_text: &InternedText<'_>,
    ) -> f32 {
        self.intrinsic_query(tree, node, axis, IntrinsicQuery::single(req), interned_text)
            .get(req)
    }

    /// Both halves — what Grid's Hug tracks want, since a track needs the
    /// content range rather than either end of it.
    #[inline]
    pub(super) fn intrinsic_range(
        &mut self,
        tree: &Tree,
        node: NodeId,
        axis: Axis,
        interned_text: &InternedText<'_>,
    ) -> IntrinsicRange {
        self.intrinsic_query(tree, node, axis, IntrinsicQuery::range(), interned_text)
    }

    /// Run measure + arrange for every root in every layer's tree
    /// against `surface` (the viewport rect). Iterates trees in
    /// `Layer::PAINT_ORDER`; each tree's recursive work receives a
    /// [`LayoutPass`] bound to that layer's output slot.
    pub(crate) fn run(
        &mut self,
        forest: &Forest,
        interned_text: &InternedText<'_>,
        surface: Rect,
        out: &mut Layout,
    ) {
        tracy::zone!();
        debug_assert_eq!(
            self.scratch.grid.depth_stack.depth, 0,
            "LayoutEngine::run entered with non-zero grid depth"
        );
        // Once per run, not per layer: `resize_for` runs inside the layer
        // loop and would wipe an earlier layer's counts.
        self.scratch.counters.begin_run();
        self.scratch.cache_rebuild =
            !Self::cache_snapshot_matches_forest(&self.cache.previous, forest, surface);
        if self.scratch.cache_rebuild {
            self.cache.begin_frame();
        }
        for layer in Layer::PAINT_ORDER {
            let tree = &forest.trees[layer];
            let layer_out = &mut out[layer];
            layer_out.resize_for(tree);
            if tree.records.is_empty() {
                continue;
            }
            self.scratch.resize_for(tree);
            {
                let mut pass = LayoutPass::new(&mut *self, tree, interned_text, &mut *layer_out);
                for slot in &tree.roots {
                    let root = slot.first_node;
                    let available = root_available(layer, slot, surface);
                    // Two of the five passes, and the only ones a Tracy
                    // capture couldn't tell apart — `PhaseSpan` already
                    // splits them for the debug overlay, so the zones go
                    // on the same boundaries rather than inventing new
                    // ones. Per root, not per node: bounded by layer
                    // count, so the zone budget stays flat.
                    let measure_span = PhaseSpan::start();
                    let desired = {
                        tracy::zone!("Layout::measure");
                        pass.measure(root, available)
                    };
                    pass.note_measure(measure_span);
                    let root_layout = tree.records.layout()[root.idx()];
                    let size = AxisPlacement::arrange_size(
                        &root_layout,
                        tree.bounds(root),
                        desired,
                        available,
                    );
                    // Overlay policies need the current measured body, not a
                    // response rect retained from an earlier frame.
                    let origin = if layer == Layer::Main {
                        surface.min
                    } else {
                        slot.placement.origin(size, surface)
                    };
                    let arrange_span = PhaseSpan::start();
                    {
                        tracy::zone!("Layout::arrange");
                        pass.arrange(root, Rect { min: origin, size });
                    }
                    pass.note_arrange(arrange_span);
                }
            }
            let capture_span = PhaseSpan::start();
            if self.scratch.cache_rebuild {
                self.cache.capture_tree(
                    tree,
                    CaptureTreeInput {
                        desired: &mut self.scratch.desired,
                        rect: &layer_out.rect,
                        scroll_content: &layer_out.scroll_content,
                        intrinsics: &self.scratch.intrinsics,
                        available_q: &mut self.scratch.available_q,
                        grid_track_state: &self.scratch.grid.track_state,
                        text_spans: &layer_out.text_spans,
                        text_shapes: &layer_out.text_shapes,
                    },
                );
            }
            self.scratch.counters.add_capture(capture_span);
            // Container text is paint-only; its wrap width exists only
            // after arrange, so it gets its own pass over the owners the
            // rollup already identified.
            let layouts = tree.records.layout();
            let mut pass = LayoutPass::new(&mut *self, tree, interned_text, &mut *layer_out);
            for index in tree.container_text.ones() {
                let layout = layouts[index];
                if !layout.meta.visibility().is_visible() {
                    continue;
                }
                let node = NodeId(index as u32);
                let available_w = (pass.rect(node).size.w - layout.padding.horiz()).max(0.0);
                let runs = TextShapeInput::on_container(tree, interned_text, node);
                pass.shape_text_runs(node, available_w, runs);
            }
        }
        let finish_span = PhaseSpan::start();
        if self.scratch.cache_rebuild {
            self.cache.finish_frame();
        }
        self.scratch.counters.add_capture(finish_span);
        debug_assert_eq!(
            self.scratch.grid.depth_stack.depth, 0,
            "LayoutEngine::run exited with non-zero grid depth"
        );
    }
}
