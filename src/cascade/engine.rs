//! The cascade walk: [`CascadeEngine`] and its scratch. The module root holds the retained product, this file the machinery that fills it.

use crate::cascade::Cascade;
use crate::cascade::cascade_input_hash::CascadeInputHash;
use crate::cascade::cascade_key::CascadeKey;
use crate::cascade::cascade_source::CascadeSource;
use crate::cascade::counters::CascadeCounters;
use crate::cascade::paint::Paint;
use crate::cascade::paint_rect::{self, PaintRectCtx};
use crate::cascade::plane_cascade::PlaneCascade;
use crate::cascade::rows::arrow_group_row::ArrowGroupRow;
use crate::cascade::rows::entry_row::EntryRow;
use crate::cascade::rows::hit_row::HitRow;
use crate::cascade::rows::root_row::RootRow;
use crate::cascade::rows::scope_row::ScopeRow;
use crate::cascade::rows::tab_stop_row::TabStopRow;
use crate::common::hash::Hasher;
use crate::common::non_max_u32::NonMaxU32;
use crate::common::tracy;
use crate::input::key_class::KeyFilter;
use crate::input::sense::Sense;
use crate::layout::plane_layout::PlaneLayout;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::geometry::translate_scale::TranslateScale;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::math::float_hash;
use crate::scene::forest::Forest;
use crate::scene::plane::Plane;
use crate::scene::tree::Tree;
use crate::scene::tree::node_id::NodeId;
use std::hash::Hasher as _;

#[derive(Clone, Copy, Debug)]
struct NodeRows {
    node: usize,
    root: bool,
    visible_rect: Rect,
    transform: TranslateScale,
    disabled: bool,
    invisible: bool,
}

impl NodeRows {
    const fn entry(self) -> EntryRow {
        EntryRow {
            rect: self.visible_rect,
            transform: self.transform,
            disabled: self.disabled,
        }
    }
}

/// Where one tree's walk puts its per-node rows; a trait so each walk is monomorphized with the other's branches folded away.
trait RowSink {
    /// Whether the walk repairs a retained cascade in place: only then may it skip nodes, and it must bail on a paint span that changed length.
    const INCREMENTAL: bool;

    /// Whether a node whose own inputs held may be skipped with its subtree; sound only while every rect in the plane held.
    fn skips_clean_subtrees(&self) -> bool;

    fn emit(&mut self, tree: &Tree, lc: &mut PlaneCascade, rows: NodeRows);
}

#[derive(Debug)]
struct TreeSink<'a> {
    entries: &'a mut Vec<EntryRow>,
    hits: &'a mut Vec<HitRow>,
    scopes: &'a mut Vec<ScopeRow>,
    tab_stops: &'a mut Vec<TabStopRow>,
    roots: &'a mut Vec<RootRow>,
    arrow_groups: &'a mut Vec<ArrowGroupRow>,
    plane: Plane,
    root: WidgetId,
}

impl RowSink for TreeSink<'_> {
    const INCREMENTAL: bool = false;

    fn skips_clean_subtrees(&self) -> bool {
        false
    }

    #[inline]
    fn emit(&mut self, tree: &Tree, lc: &mut PlaneCascade, rows: NodeRows) {
        let iu = rows.node;
        let id = tree.records.widget_id()[iu];
        let attrs = tree.records.attrs()[iu];
        let plane = self.plane;
        if rows.root {
            self.root = id;
            self.roots.push(RootRow {
                plane,
                layer: tree.root_layer(NodeId(iu as u32)),
                id,
            });
        }
        // Names the keyboard half alone; only `invisible` removes a node from pointer routing.
        let keyboard_off = rows.disabled || rows.invisible;
        // Disabling takes away what a widget may *do*, not what it occupies: it stays drawn and under the pointer, so it keeps its declared sense; the response it reads has its interaction half cleared.
        let sense = if rows.invisible {
            Sense::NONE
        } else {
            attrs.sense()
        };
        // Focus is the exception: focusing a disabled widget would put the keyboard where no key is answered.
        let focusable = !keyboard_off && attrs.is_focusable();
        if !keyboard_off && let Some(axis) = attrs.arrow_focus() {
            self.arrow_groups.push(ArrowGroupRow { id, axis });
        }
        if focusable && attrs.is_tab_stop() {
            self.tab_stops.push(TabStopRow {
                plane,
                root: self.root,
                id,
                idx: tree.bounds(NodeId(iu as u32)).tab_idx,
            });
        }
        lc.hit_rows[iu] = (sense != Sense::NONE || focusable).then(|| {
            self.hits.push(HitRow {
                rect: rows.visible_rect,
                widget_id: id,
                sense,
                focusable,
                disabled: rows.disabled,
            });
            NonMaxU32::index(self.hits.len() - 1)
        });
        let filter = if keyboard_off {
            KeyFilter::NONE
        } else {
            attrs.key_filter()
        };
        if filter.is_scope() {
            self.scopes.push(ScopeRow { plane, id, filter });
        }
        self.entries.push(rows.entry());
    }
}

/// The rows a repair rewrites in place: one plane's block of [`Cascade::entries`] and the hit table; the key proved every other table unchanged.
#[derive(Debug)]
struct TreePatch<'a> {
    entries: &'a mut [EntryRow],
    hits: &'a mut [HitRow],
    rects_kept: bool,
}

impl RowSink for TreePatch<'_> {
    const INCREMENTAL: bool = true;

    fn skips_clean_subtrees(&self) -> bool {
        self.rects_kept
    }

    /// Only the rect and inherited transform are rewritten.
    #[inline]
    fn emit(&mut self, _tree: &Tree, lc: &mut PlaneCascade, rows: NodeRows) {
        self.entries[rows.node] = rows.entry();
        if let Some(hit) = lc.hit_rows[rows.node] {
            self.hits[hit.idx()].rect = rows.visible_rect;
        }
    }
}

/// The four values a node hands its descendants, the only inputs [`Self::prefix`] hashes; bundled because two adjacent `bool`s swap silently.
#[derive(Clone, Copy, Debug)]
pub(super) struct CascadeCtx {
    pub(super) transform: TranslateScale,
    pub(super) clip: Option<Rect>,
    pub(super) disabled: bool,
    pub(super) invisible: bool,
}

impl CascadeCtx {
    pub(super) const ROOT: Self = Self {
        transform: TranslateScale::IDENTITY,
        clip: None,
        disabled: false,
        invisible: false,
    };

    /// Ancestor-derived portion of the `cascade_input` hash, folded once per frame at push and cloned per descendant. Fed as words from registers: hashing a stored struct as bytes reads stores back across seams that can't be forwarded.
    #[inline]
    pub(super) fn prefix(self) -> Hasher {
        let (clip, clip_present) = match self.clip {
            Some(rect) => (rect.canon_lanes(), true),
            None => ([0; 4], false),
        };
        let flags = u32::from(clip_present)
            | (u32::from(self.disabled) << 1)
            | (u32::from(self.invisible) << 2);
        let word = |lo: u32, hi: u32| u64::from(lo) | (u64::from(hi) << 32);
        let mut h = Hasher::new();
        h.write_u64(word(
            float_hash::canon_bits(self.transform.translation().x),
            float_hash::canon_bits(self.transform.translation().y),
        ));
        h.write_u64(word(
            float_hash::canon_bits(self.transform.scale() - 1.0),
            flags,
        ));
        h.write_u64(word(clip[0], clip[1]));
        h.write_u64(word(clip[2], clip[3]));
        h
    }
}

/// One plane's read-only input to [`CascadeEngine::run_tree`].
#[derive(Clone, Copy, Debug)]
struct TreeSource<'a> {
    tree: &'a Tree,
    layout: &'a PlaneLayout,
    display_scale: f32,
}

#[derive(Debug)]
struct Frame {
    cascade: CascadeCtx,
    subtree_end: u32,
    node_idx: usize,
    subtree_paint_rect: Rect,
    /// FxHasher state seeded with this frame's ancestor-derived inputs, cloned per descendant so the 32 B prefix isn't re-hashed.
    cascade_prefix: Hasher,
}

#[derive(Debug, Default)]
pub(crate) struct CascadeEngine {
    stack: Vec<Frame>,
    /// One node's rows on the incremental path, cleared per node: each node's rows are copied over its retained span at once, so the buffer never holds more than one node.
    paint_rows: Vec<Paint>,
    pub(crate) counters: CascadeCounters,
}

impl CascadeEngine {
    /// Brings the frozen cascade up to `source`'s key: an unchanged key skips, unchanged structure refreshes in place, else rebuild.
    pub(crate) fn run(&mut self, source: CascadeSource<'_>, cascade: &mut Cascade) {
        tracy::zone!();
        let key = CascadeKey::new(source);
        let ran = cascade.key != Some(key);
        self.counters.note_ran(ran);
        if !ran {
            return;
        }
        if !cascade.key.is_some_and(|built| built.keeps_structure(&key)) {
            self.rebuild(source, key, cascade);
            return;
        }
        debug_assert_retained_fits(source.forest, cascade);

        for (plane, tree) in source.forest.trees.iter_paint_order() {
            let n = tree.records.len();
            let rects = source.layout[plane].rect_hash();
            let lc = &mut cascade.planes[plane];
            let base = lc.entries_base as usize;
            let rects_kept = lc.rects == rects;
            lc.rects = rects;
            self.stack.clear();
            let incremental_complete = self.run_tree(
                TreeSource {
                    tree,
                    layout: &source.layout[plane],
                    display_scale: source.display.scale_factor(),
                },
                &mut cascade.planes[plane],
                &mut TreePatch {
                    entries: &mut cascade.entries[base..base + n],
                    hits: &mut cascade.hits,
                    rects_kept,
                },
            );
            if !incremental_complete {
                self.counters.abandoned_incremental();
                // Restarts at the first plane: `rebuild` assigns per-plane bases into one flat `entries` table as it walks, so it has no mid-way entry. Costs one extra paint repair.
                self.rebuild(source, key, cascade);
                return;
            }
        }
        cascade.key = Some(key);
    }

    fn rebuild(&mut self, source: CascadeSource<'_>, key: CascadeKey, cascade: &mut Cascade) {
        self.counters.full_rebuild();
        let forest = source.forest;
        let total = forest.total_nodes();
        cascade.entries.clear();
        cascade.entries.reserve_exact(total);
        cascade.hits.clear();
        cascade.scopes.clear();
        cascade.tab_stops.clear();
        cascade.roots.clear();
        cascade.arrow_groups.clear();
        cascade.planes.cover(&forest.trees);

        for (plane, tree) in forest.trees.iter_paint_order() {
            let n = tree.records.len();
            let entries_base = cascade.entries.len() as u32;
            let lc = &mut cascade.planes[plane];
            lc.reset_for(n, entries_base);
            lc.rects = source.layout[plane].rect_hash();
            self.stack.clear();
            let full_complete = self.run_tree(
                TreeSource {
                    tree,
                    layout: &source.layout[plane],
                    display_scale: source.display.scale_factor(),
                },
                &mut cascade.planes[plane],
                &mut TreeSink {
                    entries: &mut cascade.entries,
                    hits: &mut cascade.hits,
                    scopes: &mut cascade.scopes,
                    tab_stops: &mut cascade.tab_stops,
                    roots: &mut cascade.roots,
                    arrow_groups: &mut cascade.arrow_groups,
                    plane,
                    root: WidgetId::default(),
                },
            );
            debug_assert!(full_complete);
            cascade.planes[plane]
                .arena_hashes
                .copy_from_slice(&tree.rollups.subtree);
            debug_assert_eq!(
                cascade.entries.len() as u32 - entries_base,
                n as u32,
                "run_tree must emit one entry per recorded node",
            );
        }

        // `SeenIds::pre_record` clears `cur` before a relayout pass can query the preceding pass's responses.
        //
        // Refilled, not `clone_from`: `cur` and `prev` swap every frame and hashbrown's `clone_from` reallocates when bucket counts differ.
        cascade.by_id.clear();
        cascade.by_id.extend(forest.ids.recorded());
        cascade.key = Some(key);
    }
}

/// A cascade whose key keeps its structure still has one entry per node and one arena column per plane, in `forest`'s paint order.
fn debug_assert_retained_fits(forest: &Forest, cascade: &Cascade) {
    debug_assert_eq!(cascade.entries.len(), forest.total_nodes());
    let mut entries_base = 0u32;
    for (plane, tree) in forest.trees.iter_paint_order() {
        let n = tree.records.len();
        let lc = &cascade.planes[plane];
        debug_assert_eq!(lc.entries_base, entries_base);
        debug_assert_eq!(
            lc.arena_hashes.len(),
            n,
            "retained arena column outlived its plane",
        );
        entries_base += n as u32;
    }
}

/// Finalizes one stack frame: writes the rolled-up `subtree_paint_rect` and unions upward into the new top.
#[inline]
const fn finalize_frame(stack: &mut [Frame], subtree_paint_rects: &mut [Rect], popped: &Frame) {
    subtree_paint_rects[popped.node_idx] = popped.subtree_paint_rect;
    if let Some(parent) = stack.last_mut() {
        parent.subtree_paint_rect = parent.subtree_paint_rect.union(popped.subtree_paint_rect);
    }
}

impl CascadeEngine {
    /// Walks one tree, writing its cascade columns. Returns whether it *completed*; only the incremental path returns `false`, when a node's repaired paint span changes length and the caller must fully rebuild.
    ///
    /// **Incremental recompute.** A node whose `cascade_input` (inherited state and own rect) and subtree rollup both held keeps its rows; a moved transform or clip changes every child's prefix, so the change reaches exactly its subtree. A clean node is skipped with its subtree while the plane's rects held; once one moved, every node is visited. Movement within `EPS` of zero reads as held.
    fn run_tree<S: RowSink>(
        &mut self,
        source: TreeSource<'_>,
        lc: &mut PlaneCascade,
        sink: &mut S,
    ) -> bool {
        let TreeSource {
            tree,
            layout,
            display_scale,
        } = source;
        let n = tree.records.len() as u32;
        let layout_col = tree.records.layout();
        let attrs_col = tree.records.attrs();
        let ends = tree.records.subtree_end();
        let subtree_hashes = tree.rollups.subtree.as_slice();
        let skip_clean = sink.skips_clean_subtrees();
        let root_prefix = CascadeCtx::ROOT.prefix();

        let mut i: u32 = 0;
        while i < n {
            while let Some(popped) = self.stack.pop_if(|top| i >= top.subtree_end) {
                finalize_frame(&mut self.stack, &mut lc.subtree_paint_rects, &popped);
            }
            let top = self.stack.last();
            let parent = top.map_or(CascadeCtx::ROOT, |frame| frame.cascade);
            let parent_prefix = top.map_or(&root_prefix, |frame| &frame.cascade_prefix);

            let iu = i as usize;
            let id = NodeId(i);
            let attrs = attrs_col[iu];
            let layout_core = layout_col[iu];

            let disabled = parent.disabled || attrs.is_disabled();
            let invisible = parent.invisible || !layout_core.meta.visibility().is_visible();

            let layout_rect = layout.rect[iu];
            let subtree_end = ends[iu].end();
            let has_children = ends[iu].has_children(iu);
            let cascade_input = CascadeInputHash::finish(parent_prefix, layout_rect, invisible);
            let clean = S::INCREMENTAL
                && lc.cascade_inputs[iu] == cascade_input
                && lc.arena_hashes[iu] == subtree_hashes[iu];
            if clean && (skip_clean || !has_children) {
                if let Some(parent_frame) = self.stack.last_mut() {
                    parent_frame.subtree_paint_rect = parent_frame
                        .subtree_paint_rect
                        .union(lc.subtree_paint_rects[iu]);
                }
                i = subtree_end;
                continue;
            }

            let screen_rect = parent.transform.apply_rect(layout_rect);
            let visible_rect = paint_rect::clip_screen(screen_rect, parent.clip);
            // The transform descendants inherit and direct shapes paint under (`Panel::transform`): `parent ∘ self_anchored`. The `None` arm skips the compose; `compute_paint_rect` reuses it.
            let desc_transform = match tree.anchored_transform(id, layout_rect) {
                Some(t) => parent.transform.compose(t),
                None => parent.transform,
            };
            let clips = attrs.clip_mode().is_clip();
            // The encoder pushes this inner box as the clip mask before the body; clipping here keeps shape damage and child clips matching what paints (else a TextEdit's tall text shape damages past the editor on every scroll).
            let shape_clip = if clips {
                let mask_screen = parent
                    .transform
                    .apply_rect(layout_core.inner_rect(layout_rect));
                Some(paint_rect::clip_screen(mask_screen, parent.clip))
            } else {
                parent.clip
            };
            // Invisible nodes never paint; a `Rect::ZERO` rollup seed keeps a hidden subtree from inflating the ancestor's `subtree_paint_rect` and defeating the cull.
            let subtree_seed = if clean {
                lc.paint_rects[iu]
            } else {
                let ctx = PaintRectCtx {
                    tree,
                    layout,
                    node: id,
                    visible_rect,
                    parent_transform: parent.transform,
                    parent_clip: parent.clip,
                    shape_clip,
                    shape_transform: desc_transform,
                    display_scale,
                    clips,
                    has_children,
                    invisible,
                };
                let paint_rect = if S::INCREMENTAL {
                    self.counters.refreshed_node();
                    let old_span = lc.paint_arena.node_spans[iu];
                    self.paint_rows.clear();
                    let paint = ctx.compute_paint_rect(&mut self.paint_rows);
                    if old_span.len != paint.span.len {
                        return false;
                    }
                    lc.paint_arena.rows[old_span.range()].copy_from_slice(&self.paint_rows);
                    paint.rect
                } else {
                    let paint = ctx.compute_paint_rect(&mut lc.paint_arena.rows);
                    lc.paint_arena.node_spans[iu] = paint.span;
                    paint.rect
                };
                let subtree_seed = if invisible { Rect::ZERO } else { paint_rect };
                if S::INCREMENTAL {
                    lc.arena_hashes[iu] = subtree_hashes[iu];
                } else {
                    lc.subtree_ends[iu] = subtree_end;
                }
                lc.cascade_inputs[iu] = cascade_input;
                lc.paint_rects[iu] = subtree_seed;
                sink.emit(
                    tree,
                    lc,
                    NodeRows {
                        node: iu,
                        root: self.stack.is_empty(),
                        visible_rect,
                        transform: parent.transform,
                        disabled,
                        invisible,
                    },
                );
                subtree_seed
            };
            lc.subtree_paint_rects[iu] = subtree_seed;

            if has_children {
                let cascade = CascadeCtx {
                    transform: desc_transform,
                    clip: shape_clip,
                    disabled,
                    invisible,
                };
                self.stack.push(Frame {
                    cascade,
                    subtree_end,
                    node_idx: iu,
                    subtree_paint_rect: subtree_seed,
                    cascade_prefix: cascade.prefix(),
                });
            } else if let Some(parent_frame) = self.stack.last_mut() {
                // Leaf: no frame; its slot holds the seed, folded into the parent. Skips a Frame push/pop and prefix hashing.
                parent_frame.subtree_paint_rect =
                    parent_frame.subtree_paint_rect.union(subtree_seed);
            }
            i += 1;
        }
        while let Some(popped) = self.stack.pop() {
            finalize_frame(&mut self.stack, &mut lc.subtree_paint_rects, &popped);
        }
        true
    }
}

#[cfg(feature = "internals")]
pub(crate) mod internals {
    use crate::cascade::Cascade;
    use crate::cascade::cascade_key::CascadeKey;
    use crate::cascade::cascade_source::CascadeSource;
    use crate::cascade::engine::CascadeEngine;
    #[cfg(test)]
    use crate::cascade::paint::Paint;
    #[cfg(test)]
    use crate::internals::scratch_report;

    impl CascadeEngine {
        /// Rebuilds every row from `source`, whatever the cascade holds.
        pub(crate) fn run_full(&mut self, source: CascadeSource<'_>, cascade: &mut Cascade) {
            self.rebuild(source, CascadeKey::new(source), cascade);
        }

        /// The last node's rows the incremental walk recomputed.
        #[cfg(test)]
        pub(crate) fn paint_rows(&self) -> &[Paint] {
            &self.paint_rows
        }

        #[cfg(test)]
        pub(crate) fn scratch_bytes(&self) -> usize {
            let Self {
                stack,
                paint_rows,
                counters: _,
            } = self;
            scratch_report::vec_bytes(stack) + scratch_report::vec_bytes(paint_rows)
        }
    }
}
