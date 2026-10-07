//! The layout walk: [`LayoutEngine`], its between-frame scratch, and the
//! snapshot check deciding whether last frame's measurements still apply.

use crate::common::tracy;
use crate::layout::Layout;
use crate::layout::axis_placement::{AxisPlacement, Placed};
use crate::layout::cache::{CaptureTreeInput, MeasureCache};
use crate::layout::counters::PhaseSpan;
use crate::layout::intrinsic::intrinsic_query::IntrinsicQuery;
use crate::layout::intrinsic::intrinsic_range::IntrinsicRange;
use crate::layout::intrinsic::len_req::LenReq;
use crate::layout::layout_scratch::LayoutScratch;
use crate::layout::pass::LayoutPass;
use crate::layout::text::text_shape_input::TextShapeInput;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::geometry::size::Size;
use crate::primitives::layout::axis::Axis;
use crate::primitives::layout::layout_mode::LayoutMode;
use crate::primitives::text::interned_text::InternedText;
use crate::scene::forest::Forest;
use crate::scene::layer::Layer;
use crate::scene::tree::Tree;
use crate::scene::tree::node_id::NodeId;
use crate::text::shaper::TextShaper;
use crate::text::system::TextSystem;

/// Persistent layout engine. Field groups by lifetime:
///
/// - `scratch`: per-frame state (see [`LayoutScratch`]), reset per layer by
///   `LayoutScratch::resize_for`.
/// - `text`: per-window text shaping and reuse slots.
/// - `cache`: cross-frame measure cache, see [`crate::layout::cache`].
/// - `last_roots`: how the last run laid each root out, which with the
///   snapshot decides whether its output is this run's.
///
/// Per-frame output is not held here: `run` threads it through an
/// `out: &mut Layout` owned by the caller. Recursive work receives only the
/// current [`LayerLayout`](crate::layout::layer_layout::LayerLayout) slot.
#[derive(Debug)]
pub(crate) struct LayoutEngine {
    pub(crate) scratch: LayoutScratch,
    pub(crate) text: TextSystem,
    pub(crate) cache: MeasureCache,
    last_roots: Vec<RootRun>,
}

/// One root as a run laid it out: its layer, the exact extent it was measured
/// against, and the slot it was arranged into (outside its margin). Kept in
/// paint order.
#[derive(Clone, Copy, Debug)]
struct RootRun {
    layer: Layer,
    available: Size,
    slot: Rect,
}

impl LayoutEngine {
    pub(crate) fn new(shaper: TextShaper) -> Self {
        Self {
            scratch: LayoutScratch::default(),
            text: TextSystem::new(shaper),
            cache: MeasureCache::default(),
            last_roots: Vec::new(),
        }
    }

    /// Whether the last run's output is what this run would write, so `out`
    /// stands as it is.
    ///
    /// The snapshot match proves each root's subtree is the one last laid out.
    /// The offer, which the snapshot keeps only quantized, is compared exactly
    /// here with the layer. Equal subtree and offer arrange to the same slot
    /// size, so the slot holds iff the origin placement resolves to is where
    /// the last run put it. Equal inputs give an equal run.
    fn keeps_last_run(&self, forest: &Forest, surface: Rect) -> bool {
        let mut last = self.last_roots.iter();
        for (layer, tree) in forest.trees.iter_paint_order() {
            for slot in &tree.roots {
                let Some(run) = last.next() else {
                    return false;
                };
                if run.layer != layer
                    || run.available != slot.available(layer, surface)
                    || run.slot.min != slot.origin(layer, run.slot.size, surface)
                {
                    return false;
                }
            }
        }
        last.next().is_none()
    }

    /// Grid's per-track intrinsic aggregator: a bump stack `Grid::intrinsic`
    /// extends, recurses through and truncates back. Reached by name like
    /// [`LayoutPass`]'s accessors; the intrinsic query stays off the pass.
    #[inline]
    pub(super) const fn grid_track_aggregator(&mut self) -> &mut Vec<f32> {
        &mut self.scratch.grid.track_aggregator
    }

    /// Cross-frame intrinsic for one `(node, axis, req)` slot, or `None` when
    /// the node is ineligible or the snapshot has no value. Only non-leaf nodes
    /// are cacheable (a leaf's is cheap and owns no descriptor). Intrinsics are
    /// independent of `available`, so this checks `subtree_hash` alone and hits
    /// even on a resize frame where `try_lookup` misses.
    #[inline]
    fn cached_intrinsic(&self, tree: &Tree, idx: usize, slot: usize) -> Option<f32> {
        if LayoutMode::from(tree.records.layout()[idx].meta) == LayoutMode::Leaf {
            return None;
        }
        self.cache.lookup_root_intrinsic(
            tree.records.widget_id()[idx],
            tree.rollups.layout_subtree[idx],
            slot,
        )
    }

    /// On-demand intrinsic-size query: outer (margin-inclusive) size on `axis`
    /// for the halves `query` asks for.
    ///
    /// A pure function of the subtree at `node`, independent of the parent's
    /// available width and the arranged rect. Three layers answer, cheapest
    /// first: the intra-frame slot array, last frame's snapshot, then a real
    /// subtree walk. Only halves still missing reach the walk.
    ///
    /// A walk that also covered the other axis (see
    /// [`IntrinsicWalk`](crate::layout::intrinsic::intrinsic_walk::IntrinsicWalk))
    /// is recorded there too, keeping `measure`'s pair of min-content queries
    /// to one pass over a leaf's text runs.
    ///
    /// Consumed by `Grid::measure` and `Stack::measure` via the thin
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
            // Cross-frame reuse from last frame's snapshot, which hits even
            // on a resize frame. It fires at the query site because a parent
            // computes `intrinsic_min` before measuring its children, so their
            // own cache-hit restore comes too late to stop the ancestor
            // recursing through every unchanged sibling subtree.
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
        let computed = walk.walk(self, tree, node, axis, interned_text);
        if let Some(sibling) = computed.sibling {
            self.record_intrinsic(idx, axis.other(), walk, sibling);
        }
        self.record_intrinsic(idx, axis, walk, computed.answered);
        for (req, slot) in range.requested(walk) {
            *slot = computed.answered.get(req);
        }
        range
    }

    /// Store the halves `query` names of `found` in this frame's slot array.
    #[inline]
    fn record_intrinsic(
        &mut self,
        idx: usize,
        axis: Axis,
        query: IntrinsicQuery,
        mut found: IntrinsicRange,
    ) {
        for (req, value) in found.requested(query) {
            self.scratch.intrinsics[idx][req.slot(axis)] = *value;
        }
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

    /// Both halves, as Grid's Hug tracks need the content range.
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

    /// Run measure and arrange for every root in every layer's tree against
    /// `surface`, in `Layer::PAINT_ORDER`; each tree's recursive work gets a
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
        // Once per run: `resize_for` runs inside the layer loop and would wipe earlier counts.
        self.scratch.counters.begin_pass();
        // Before the snapshot check, which cannot see what this answers:
        // `TextSystem::sync_fonts` drops the reuse rows and reports the frame
        // the snapshot must go on.
        if self.text.sync_fonts() {
            self.cache.forget_all();
        }
        self.scratch.cache_rebuild = !self.cache.matches_forest(forest, surface);
        if !self.scratch.cache_rebuild && self.keeps_last_run(forest, surface) {
            self.scratch.counters.kept_last_run();
            return;
        }
        if self.scratch.cache_rebuild {
            self.cache.begin_frame();
        }
        self.last_roots.clear();
        for layer in Layer::PAINT_ORDER {
            let tree = &forest.trees[layer];
            let layer_out = &mut out[layer];
            layer_out.resize_for(tree);
            if tree.records.is_empty() {
                layer_out.hash_rects();
                continue;
            }
            self.scratch.resize_for(tree);
            for slot in &tree.roots {
                let mut pass = LayoutPass::new(&mut *self, tree, interned_text, &mut *layer_out);
                let root = slot.first_node;
                let available = slot.available(layer, surface);
                // Two of the five passes that a Tracy capture couldn't tell
                // apart; zones follow `PhaseSpan`'s boundaries. Per root, so
                // the zone budget stays flat.
                let measure_span = PhaseSpan::start();
                let measured = {
                    tracy::zone!("Layout::measure");
                    pass.measure(root, available)
                };
                pass.note_measure(measure_span);
                let root_layout = tree.records.layout()[root.idx()];
                let size = AxisPlacement::arrange_size(
                    &root_layout,
                    tree.bounds(root),
                    Placed::of(measured.size, measured.floor),
                    available,
                );
                // Overlay policies need the current measured body, not a retained response rect.
                let arranged = Rect {
                    min: slot.origin(layer, size, surface),
                    size,
                };
                let arrange_span = PhaseSpan::start();
                {
                    tracy::zone!("Layout::arrange");
                    pass.arrange(root, arranged);
                }
                pass.note_arrange(arrange_span);
                self.last_roots.push(RootRun {
                    layer,
                    available,
                    slot: arranged,
                });
            }
            let capture_span = PhaseSpan::start();
            if self.scratch.cache_rebuild {
                self.cache.capture_tree(
                    tree,
                    CaptureTreeInput {
                        desired: &self.scratch.desired,
                        floor: &self.scratch.floor,
                        stable_from: &self.scratch.stable_from,
                        rect: &layer_out.rect,
                        local: &self.scratch.local,
                        scroll_content: &layer_out.scroll_content,
                        intrinsics: &self.scratch.intrinsics,
                        available_q: &self.scratch.available_q,
                        grid_track_state: &self.scratch.grid.track_state,
                        text_spans: &layer_out.text_spans,
                        text_shapes: &layer_out.text_shapes,
                    },
                );
            }
            self.scratch.counters.add_capture(capture_span);
            // Container text is paint-only and its wrap width exists only after
            // arrange, so it gets its own pass over the identified owners.
            let layouts = tree.records.layout();
            let mut pass = LayoutPass::new(&mut *self, tree, interned_text, &mut *layer_out);
            for index in tree.container_text.ones() {
                let layout = layouts[index];
                let node = NodeId(index as u32);
                // The inner box `arrange` places children in.
                let available_w = layout.inner_rect(pass.rect(node)).size.w;
                let runs = TextShapeInput::on_container(tree, interned_text, node);
                pass.shape_text_runs(node, available_w, runs);
            }
            out[layer].hash_rects();
        }
        let finish_span = PhaseSpan::start();
        if self.scratch.cache_rebuild {
            self.cache.end_frame();
        }
        self.scratch.counters.add_capture(finish_span);
        debug_assert_eq!(
            self.scratch.grid.depth_stack.depth, 0,
            "LayoutEngine::run exited with non-zero grid depth"
        );
    }
}

#[cfg(any(test, feature = "bench"))]
pub(crate) mod internals {
    use crate::layout::engine::LayoutEngine;
    #[cfg(test)]
    use crate::layout::intrinsic::len_req::{LenReq, SLOT_COUNT};
    #[cfg(test)]
    use crate::primitives::layout::axis::Axis;
    #[cfg(test)]
    use crate::scene::forest::Forest;
    #[cfg(test)]
    use crate::scene::layer::Layer;
    #[cfg(test)]
    use crate::scene::tree::node_id::NodeId;

    impl LayoutEngine {
        /// [`Self::intrinsic`] on `forest`'s main tree, interning its text.
        #[cfg(test)]
        pub(crate) fn main_intrinsic(
            &mut self,
            forest: &Forest,
            node: NodeId,
            axis: Axis,
            req: LenReq,
        ) -> f32 {
            let interned_text = forest.record_store.interned_text();
            self.intrinsic(&forest.trees[Layer::Main], node, axis, req, &interned_text)
        }

        /// Make the next run lay the forest out again, to drive the snapshot
        /// restore on a frame that changes nothing.
        pub(crate) fn forget_last_run(&mut self) {
            self.last_roots.clear();
        }

        /// Drop every cached intrinsic and zero the compute counter.
        #[cfg(test)]
        pub(crate) fn forget_intrinsics(&mut self) {
            self.scratch.intrinsics.fill([f32::NAN; SLOT_COUNT]);
            self.scratch.counters.reset_intrinsic_computes();
        }
    }
}
