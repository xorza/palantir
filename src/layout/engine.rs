//! The layout walk: [`LayoutEngine`], its between-frame scratch, and the
//! snapshot check deciding whether last frame's measurements still apply.

use crate::common::tracy;
use crate::layout::Layout;
use crate::layout::cache::{CaptureTreeInput, MeasureCache};
use crate::layout::layer_input::LayerInput;
use crate::layout::layout_scratch::LayoutScratch;
use crate::layout::pass::LayoutPass;
use crate::layout::placed_child::{Placed, PlacedChild};
use crate::layout::text::text_shape_input::TextShapeInput;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::geometry::size::Size;
use crate::scene::forest::Forest;
use crate::scene::layer::Layer;
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

    /// Run measure and arrange for every root in every layer's tree against
    /// `surface`, in `Layer::PAINT_ORDER`; each tree's recursive work gets a
    /// [`LayoutPass`] bound to that layer's output slot.
    pub(crate) fn run(&mut self, forest: &Forest, surface: Rect, out: &mut Layout) {
        tracy::zone!();
        let interned_text = forest.record_store.interned_text();
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
            let input = LayerInput {
                tree,
                interned_text: &interned_text,
            };
            let layer_out = &mut out[layer];
            layer_out.resize_for(tree);
            if tree.records.is_empty() {
                layer_out.hash_rects();
                continue;
            }
            self.scratch.resize_for(tree);
            for slot in &tree.roots {
                let mut pass = LayoutPass::new(&mut *self, input, &mut *layer_out);
                let root = slot.first_node;
                let available = slot.available(layer, surface);
                // Two of the five passes that a Tracy capture couldn't tell
                // apart. Per root, so the zone budget stays flat.
                let measured = {
                    tracy::zone!("Layout::measure");
                    pass.measure(root, available)
                };
                let size = PlacedChild {
                    layout: &tree.records.layout()[root.idx()],
                    bounds: tree.bounds(root),
                    measured: Placed::of(measured.size, measured.floor),
                }
                .arrange_size(available);
                // Overlay policies need the current measured body, not a retained response rect.
                let arranged = Rect {
                    min: slot.origin(layer, size, surface),
                    size,
                };
                {
                    tracy::zone!("Layout::arrange");
                    pass.arrange(root, arranged);
                }
                self.last_roots.push(RootRun {
                    layer,
                    available,
                    slot: arranged,
                });
            }
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
            // Container text is paint-only and its wrap width exists only after
            // arrange, so it gets its own pass over the identified owners.
            let layouts = tree.records.layout();
            let mut pass = LayoutPass::new(&mut *self, input, &mut *layer_out);
            for index in tree.container_text.ones() {
                let layout = layouts[index];
                let node = NodeId(index as u32);
                // The inner box `arrange` places children in.
                let available_w = layout.inner_rect(pass.rect(node)).size.w;
                let runs = TextShapeInput::on_container(tree, &interned_text, node);
                pass.shape_text_runs(node, available_w, runs);
            }
            out[layer].hash_rects();
        }
        if self.scratch.cache_rebuild {
            self.cache.end_frame();
        }
        debug_assert_eq!(
            self.scratch.grid.depth_stack.depth, 0,
            "LayoutEngine::run exited with non-zero grid depth"
        );
    }
}

#[cfg(feature = "internals")]
pub(crate) mod internals {
    use crate::layout::engine::LayoutEngine;
    #[cfg(test)]
    use crate::layout::intrinsic::intrinsic_pass::IntrinsicPass;
    #[cfg(test)]
    use crate::layout::intrinsic::intrinsic_query::IntrinsicQuery;
    #[cfg(test)]
    use crate::layout::intrinsic::intrinsic_range::IntrinsicRange;
    #[cfg(test)]
    use crate::layout::intrinsic::len_req::SLOT_COUNT;
    #[cfg(test)]
    use crate::layout::layer_input::LayerInput;
    #[cfg(test)]
    use crate::scene::forest::Forest;
    #[cfg(test)]
    use crate::scene::layer::Layer;
    #[cfg(test)]
    use crate::scene::tree::node_id::NodeId;

    impl LayoutEngine {
        /// [`IntrinsicPass::query`] on `forest`'s main tree, interning its text.
        #[cfg(test)]
        pub(crate) fn main_intrinsic(
            &mut self,
            forest: &Forest,
            node: NodeId,
            query: IntrinsicQuery,
        ) -> IntrinsicRange {
            let interned_text = forest.record_store.interned_text();
            let input = LayerInput {
                tree: &forest.trees[Layer::Main],
                interned_text: &interned_text,
            };
            IntrinsicPass::new(self, input).query(node, query)
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
