//! The layout walk: [`LayoutEngine`], its between-frame scratch, and the
//! snapshot check deciding whether last frame's measurements still apply.

use crate::common::tracy;
use crate::layout::Layout;
use crate::layout::cache::measure_snapshot::SubtreeKey;
use crate::layout::cache::{CaptureTreeInput, MeasureCache};
use crate::layout::layout_scratch::LayoutScratch;
use crate::layout::pass::LayoutPass;
use crate::layout::placed_child::{Placed, PlacedChild};
use crate::layout::plane_input::PlaneInput;
use crate::layout::text::text_shape_input::TextShapeInput;
use crate::primitives::geometry::rect::Rect;
use crate::scene::forest::Forest;
use crate::scene::plane::Plane;
use crate::scene::tree::node_id::NodeId;
use crate::text::shaper::TextShaper;
use crate::text::system::TextSystem;

/// Persistent layout engine. Field groups by lifetime:
///
/// - `scratch`: per-frame state (see [`LayoutScratch`]), reset per plane by
///   `LayoutScratch::resize_for`.
/// - `text`: per-window text shaping and reuse slots.
/// - `cache`: cross-frame measure cache, see [`crate::layout::cache`].
/// - `last_roots`: how the last run laid each root out, which decides whether
///   the snapshot still describes the forest and whether the last output is
///   this run's.
///
/// Per-frame output is not held here: `run` threads it through an
/// `out: &mut Layout` owned by the caller. Recursive work receives only the
/// current [`PlaneLayout`](crate::layout::plane_layout::PlaneLayout) slot.
#[derive(Debug)]
pub(crate) struct LayoutEngine {
    pub(crate) scratch: LayoutScratch,
    pub(crate) text: TextSystem,
    pub(crate) cache: MeasureCache,
    last_roots: Vec<RootRun>,
}

/// One root as a run laid it out: its subtree and the exact extent it was measured
/// against, its plane, and the slot it was arranged into (outside its margin). Kept in
/// paint order.
#[derive(Clone, Copy, Debug)]
struct RootRun {
    key: SubtreeKey,
    plane: Plane,
    slot: Rect,
}

/// How this run's roots compare with the last run's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LastRun {
    /// A root's subtree or offer differs, or the roots do: last frame's measures may not apply.
    Differs,
    /// Every subtree and offer is the last run's, but a root moved plane or place.
    Moved,
    /// The last run's output is this run's.
    Same,
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

    /// How `forest`'s roots at `surface` compare with the last run's.
    ///
    /// Equal subtree and offer arrange to the same slot size, so the slot holds
    /// iff the origin placement resolves to is where the last run put it. Equal
    /// inputs give an equal run.
    fn compare_last_run(&self, forest: &Forest, surface: Rect) -> LastRun {
        let mut last = self.last_roots.iter();
        let mut same = LastRun::Same;
        for (plane, tree) in forest.trees.iter_paint_order() {
            for slot in &tree.roots {
                let Some(run) = last.next() else {
                    return LastRun::Differs;
                };
                if !run.key.matches(SubtreeKey::of_root(tree, slot, surface)) {
                    return LastRun::Differs;
                }
                if run.plane != plane || run.slot.min != slot.origin(run.slot.size, surface) {
                    same = LastRun::Moved;
                }
            }
        }
        if last.next().is_some() {
            return LastRun::Differs;
        }
        same
    }

    /// Run measure and arrange for every root in every plane's tree against
    /// `surface`, in paint order; each tree's recursive work gets a
    /// [`LayoutPass`] bound to that plane's output slot.
    pub(crate) fn run(&mut self, forest: &Forest, surface: Rect, out: &mut Layout) {
        tracy::zone!();
        // First, so a run kept whole still leaves a slot for a plane that grew empty: the
        // cascade and the encoder index every plane the forest holds.
        out.cover(forest);
        let interned_text = forest.record_store.interned_text();
        debug_assert_eq!(
            self.scratch.grid.depth_stack.depth, 0,
            "LayoutEngine::run entered with non-zero grid depth"
        );
        // Once per run: `resize_for` runs inside the plane loop and would wipe earlier counts.
        self.scratch.counters.begin_pass();
        // Before the snapshot check, which cannot see what this answers:
        // `TextSystem::sync_fonts` drops the reuse rows and reports the frame
        // the snapshot must go on.
        if self.text.sync_fonts() {
            self.cache.forget_all();
        }
        let last_run = self.compare_last_run(forest, surface);
        self.scratch.cache_rebuild =
            last_run == LastRun::Differs || !self.cache.holds_nodes(forest.total_nodes());
        if !self.scratch.cache_rebuild && last_run == LastRun::Same {
            self.scratch.counters.kept_last_run();
            return;
        }
        if self.scratch.cache_rebuild {
            self.cache.begin_frame();
        }
        self.last_roots.clear();
        for (plane, tree) in forest.trees.iter_paint_order() {
            let input = PlaneInput {
                tree,
                interned_text: &interned_text,
            };
            let plane_out = &mut out[plane];
            plane_out.resize_for(tree);
            if tree.records.is_empty() {
                plane_out.hash_rects();
                continue;
            }
            self.scratch.resize_for(tree);
            for slot in &tree.roots {
                let mut pass = LayoutPass::new(&mut *self, input, &mut *plane_out);
                let root = slot.first_node;
                let available = slot.available(surface);
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
                    min: slot.origin(size, surface),
                    size,
                };
                {
                    tracy::zone!("Layout::arrange");
                    pass.arrange(root, arranged);
                }
                self.last_roots.push(RootRun {
                    key: SubtreeKey::of_root(tree, slot, surface),
                    plane,
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
                        rect: &plane_out.rect,
                        local: &self.scratch.local,
                        scroll_content: &plane_out.scroll_content,
                        intrinsics: &self.scratch.intrinsics,
                        available: &self.scratch.available,
                        grid_track_state: &self.scratch.grid.track_state,
                        text_spans: &plane_out.text_spans,
                        text_shapes: &plane_out.text_shapes,
                    },
                );
            }
            // Container text is paint-only and its wrap width exists only after
            // arrange, so it gets its own pass over the identified owners.
            let layouts = tree.records.layout();
            let mut pass = LayoutPass::new(&mut *self, input, &mut *plane_out);
            for idx in tree.container_text.ones() {
                let layout = layouts[idx];
                let node = NodeId(idx as u32);
                // The inner box `arrange` places children in.
                let available_w = layout.inner_rect(pass.rect(node)).size.w;
                let runs = TextShapeInput::on_container(tree, &interned_text, node);
                pass.shape_text_runs(node, available_w, runs);
            }
            out[plane].hash_rects();
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
    use crate::layout::plane_input::PlaneInput;
    #[cfg(test)]
    use crate::scene::forest::Forest;
    #[cfg(test)]
    use crate::scene::plane::Plane;
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
            let input = PlaneInput {
                tree: &forest.trees[Plane::MAIN],
                interned_text: &interned_text,
            };
            IntrinsicPass::new(self, input).query(node, query)
        }

        /// Make the next run lay the forest out again, to drive the snapshot
        /// restore on a frame that changes nothing: every root's slot moves to a
        /// place no origin equals, which keeps the subtrees and so the snapshot.
        pub(crate) fn forget_last_run(&mut self) {
            for run in &mut self.last_roots {
                run.slot.min = glam::Vec2::NAN;
            }
        }

        /// Drop every cached intrinsic and zero the compute counter.
        #[cfg(test)]
        pub(crate) fn forget_intrinsics(&mut self) {
            self.scratch.intrinsics.fill([f32::NAN; SLOT_COUNT]);
            self.scratch.counters.reset_intrinsic_computes();
        }
    }
}
