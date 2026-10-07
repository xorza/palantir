//! The cascade walk: [`CascadeEngine`] and its scratch. The module root holds the retained product, this file the machinery that fills it.

use crate::cascade::Cascade;
use crate::cascade::cascade_input_hash::CascadeInputHash;
use crate::cascade::cascade_key::CascadeKey;
use crate::cascade::counters::CascadeCounters;
use crate::cascade::entry::{ArrowGroupRow, EntryRow, HitRow, RootRow, ScopeRow, TabStopRow};
use crate::cascade::layer_cascade::LayerCascade;
use crate::cascade::paint::PaintArena;
use crate::cascade::paint_rect::{self, PaintRectCtx};
use crate::common::hash::Hasher;
use crate::common::span::Span;
use crate::common::tracy;
use crate::display::Display;
use crate::input::key_class::KeyFilter;
use crate::input::sense::Sense;
use crate::layout::Layout;
use crate::layout::layer_layout::LayerLayout;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::geometry::translate_scale::TranslateScale;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::math::float_hash::{self, FloatHash};
use crate::scene::forest::Forest;
use crate::scene::layer::Layer;
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

    /// Whether a node whose own inputs held may be skipped with its subtree; sound only while every rect in the layer held.
    fn skips_clean_subtrees(&self) -> bool;

    fn emit(&mut self, tree: &Tree, lc: &mut LayerCascade, rows: NodeRows);
}

#[derive(Debug)]
struct TreeSink<'a> {
    entries: &'a mut Vec<EntryRow>,
    hits: &'a mut Vec<HitRow>,
    scopes: &'a mut Vec<ScopeRow>,
    tab_stops: &'a mut Vec<TabStopRow>,
    roots: &'a mut Vec<RootRow>,
    arrow_groups: &'a mut Vec<ArrowGroupRow>,
    layer: Layer,
    root: WidgetId,
}

impl RowSink for TreeSink<'_> {
    const INCREMENTAL: bool = false;

    fn skips_clean_subtrees(&self) -> bool {
        false
    }

    #[inline]
    fn emit(&mut self, tree: &Tree, lc: &mut LayerCascade, rows: NodeRows) {
        let iu = rows.node;
        let id = tree.records.widget_id()[iu];
        let attrs = tree.records.attrs()[iu];
        let layer = self.layer;
        if rows.root {
            self.root = id;
            self.roots.push(RootRow { layer, id });
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
                layer,
                root: self.root,
                id,
                index: tree.bounds(NodeId(iu as u32)).tab_index,
            });
        }
        lc.hit_rows[iu] = if sense != Sense::NONE || focusable {
            self.hits.push(HitRow {
                rect: rows.visible_rect,
                widget_id: id,
                sense,
                focusable,
                disabled: rows.disabled,
            });
            (self.hits.len() - 1) as u32
        } else {
            LayerCascade::NO_HIT_ROW
        };
        let filter = if keyboard_off {
            KeyFilter::NONE
        } else {
            attrs.key_filter()
        };
        if filter.is_scope() {
            self.scopes.push(ScopeRow { layer, id, filter });
        }
        self.entries.push(rows.entry());
    }
}

/// The rows a repair rewrites in place: one layer's block of [`Cascade::entries`] and the hit table; the key proved every other table unchanged.
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
    fn emit(&mut self, _tree: &Tree, lc: &mut LayerCascade, rows: NodeRows) {
        self.entries[rows.node] = rows.entry();
        let hit = lc.hit_rows[rows.node];
        if hit != LayerCascade::NO_HIT_ROW {
            self.hits[hit as usize].rect = rows.visible_rect;
        }
    }
}

/// The four values a node hands its descendants, the only inputs [`build_cascade_prefix`] hashes; bundled because two adjacent `bool`s swap silently.
#[derive(Clone, Copy, Debug)]
pub(super) struct CascadeContext {
    pub(super) transform: TranslateScale,
    pub(super) clip: Option<Rect>,
    pub(super) disabled: bool,
    pub(super) invisible: bool,
}

impl CascadeContext {
    pub(super) const ROOT: Self = Self {
        transform: TranslateScale::IDENTITY,
        clip: None,
        disabled: false,
        invisible: false,
    };
}

#[derive(Debug)]
struct Frame {
    cascade: CascadeContext,
    subtree_end: u32,
    node_idx: usize,
    subtree_paint_rect: Rect,
    /// FxHasher state seeded with this frame's ancestor-derived inputs, cloned per descendant so the 32 B prefix isn't re-hashed.
    cascade_prefix: Hasher,
}

#[derive(Debug, Default)]
pub(crate) struct CascadeEngine {
    stack: Vec<Frame>,
    paint_scratch: PaintArena,
    pub(crate) counters: CascadeCounters,
}

impl CascadeEngine {
    /// Brings the frozen cascade up to `key`: unchanged key skips, unchanged structure refreshes in place, else rebuild.
    pub(crate) fn run(
        &mut self,
        forest: &Forest,
        layout: &Layout,
        display: Display,
        key: &CascadeKey,
        cascade: &mut Cascade,
    ) {
        tracy::zone!();
        let ran = cascade.key.as_ref() != Some(key);
        self.counters.note_ran(ran);
        if !ran {
            return;
        }
        let Some(built) = cascade
            .key
            .filter(|built| self.can_update(forest, built, key, cascade))
        else {
            self.run_full(forest, layout, display, key, cascade);
            return;
        };

        for (layer, tree) in forest.trees.iter_paint_order() {
            let n = tree.records.len();
            let base = cascade.layers[layer].entries_base as usize;
            self.stack.clear();
            self.paint_scratch.reset_for(n);
            let incremental_complete = self.run_tree(
                tree,
                &layout[layer],
                &mut cascade.layers[layer],
                &mut TreePatch {
                    entries: &mut cascade.entries[base..base + n],
                    hits: &mut cascade.hits,
                    rects_kept: built.keeps_rects(key, layer),
                },
                display.scale_factor(),
            );
            if !incremental_complete {
                self.counters.abandoned_incremental();
                // Restarts at layer zero: `run_full` assigns per-layer bases into one flat `entries` table as it walks, so it has no mid-way entry. Costs one extra paint repair.
                self.run_full(forest, layout, display, key, cascade);
                return;
            }
        }
        cascade.key = Some(*key);
    }

    /// Whether every retained structural table remains valid; the incremental walk refreshes the rest.
    fn can_update(
        &self,
        forest: &Forest,
        built: &CascadeKey,
        key: &CascadeKey,
        cascade: &Cascade,
    ) -> bool {
        if !built.keeps_structure(key) {
            return false;
        }
        debug_assert_eq!(cascade.entries.len(), forest.total_nodes());
        let mut entries_base = 0u32;
        for (layer, tree) in forest.trees.iter_paint_order() {
            let n = tree.records.len();
            let lc = &cascade.layers[layer];
            debug_assert_eq!(lc.entries_base, entries_base);
            debug_assert_eq!(
                lc.arena_hashes.len(),
                n,
                "retained arena column outlived its layer",
            );
            entries_base += n as u32;
        }
        true
    }

    pub(super) fn run_full(
        &mut self,
        forest: &Forest,
        layout: &Layout,
        display: Display,
        key: &CascadeKey,
        cascade: &mut Cascade,
    ) {
        self.counters.full_rebuild();
        let total = forest.total_nodes();
        cascade.entries.clear();
        cascade.entries.reserve_exact(total);
        cascade.hits.clear();
        cascade.scopes.clear();
        cascade.tab_stops.clear();
        cascade.roots.clear();
        cascade.arrow_groups.clear();

        for (layer, tree) in forest.trees.iter_paint_order() {
            let n = tree.records.len();
            let entries_base = cascade.entries.len() as u32;
            cascade.layers[layer].reset_for(n, entries_base);
            self.stack.clear();
            let full_complete = self.run_tree(
                tree,
                &layout[layer],
                &mut cascade.layers[layer],
                &mut TreeSink {
                    entries: &mut cascade.entries,
                    hits: &mut cascade.hits,
                    scopes: &mut cascade.scopes,
                    tab_stops: &mut cascade.tab_stops,
                    roots: &mut cascade.roots,
                    arrow_groups: &mut cascade.arrow_groups,
                    layer,
                    root: WidgetId::default(),
                },
                display.scale_factor(),
            );
            debug_assert!(full_complete);
            cascade.layers[layer]
                .arena_hashes
                .copy_from_slice(&tree.rollups.subtree);
            debug_assert_eq!(
                cascade.entries.len() as u32 - entries_base,
                n as u32,
                "run_tree must emit one entry per recorded node",
            );
        }

        // `SeenIds::pre_record` clears `curr` before a relayout pass can query the preceding pass's responses.
        //
        // Refilled, not `clone_from`: `curr` and `prev` swap every frame and hashbrown's `clone_from` reallocates when bucket counts differ.
        cascade.by_id.clear();
        cascade.by_id.extend(forest.ids.recorded());
        cascade.key = Some(*key);
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
    /// **Incremental recompute.** A node whose `cascade_input` (inherited state and own rect) and subtree rollup both held keeps its rows; a moved transform or clip changes every child's prefix, so the change reaches exactly its subtree. A clean node is skipped with its subtree while the layer's rects held; once one moved, every node is visited. Movement within `EPS` of zero reads as held.
    fn run_tree<S: RowSink>(
        &mut self,
        tree: &Tree,
        layout: &LayerLayout,
        lc: &mut LayerCascade,
        sink: &mut S,
        display_scale: f32,
    ) -> bool {
        let n = tree.records.len() as u32;
        let layout_col = tree.records.layout();
        let attrs_col = tree.records.attrs();
        let ends = tree.records.subtree_end();
        let subtree_hashes = tree.rollups.subtree.as_slice();
        let skip_clean = sink.skips_clean_subtrees();
        let root_prefix = build_cascade_prefix(CascadeContext::ROOT);

        let mut i: u32 = 0;
        while i < n {
            while let Some(popped) = self.stack.pop_if(|top| i >= top.subtree_end) {
                finalize_frame(&mut self.stack, &mut lc.subtree_paint_rects, &popped);
            }
            let top = self.stack.last();
            let parent = top.map_or(CascadeContext::ROOT, |frame| frame.cascade);
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
            let cascade_input = finish_cascade_input(parent_prefix, layout_rect, invisible);
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
                };
                let paint_rect = if S::INCREMENTAL {
                    self.counters.refreshed_node();
                    let old_span = lc.paint_arena.node_spans[iu];
                    let paint_rect = compute_node_paint(ctx, invisible, &mut self.paint_scratch);
                    let new_span = self.paint_scratch.node_spans[iu];
                    if old_span.len != new_span.len {
                        return false;
                    }
                    lc.paint_arena.rows[old_span.range()]
                        .copy_from_slice(&self.paint_scratch.rows[new_span.range()]);
                    paint_rect
                } else {
                    compute_node_paint(ctx, invisible, &mut lc.paint_arena)
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
                let cascade = CascadeContext {
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
                    cascade_prefix: build_cascade_prefix(cascade),
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

/// The node's paint rows and extent, or an empty span when nothing here can reach the surface.
///
/// The flag is the cascaded reading: a subtree under a non-`Visible` ancestor paints nothing. Damage reads the emptied span as a rows-to-rowless transition. `Tree::container_text` uses the same reading, which [`TextRuns`](crate::layout::text::text_runs::TextRuns)' text-record pairing depends on.
#[inline]
fn compute_node_paint(ctx: PaintRectCtx<'_>, invisible: bool, arena: &mut PaintArena) -> Rect {
    if invisible {
        arena.node_spans[ctx.node.idx()] = Span::new(arena.rows.len() as u32, 0);
        return Rect::ZERO;
    }
    paint_rect::compute_paint_rect(ctx, arena)
}

/// Ancestor-derived portion of the `cascade_input` hash, folded once per frame at push and cloned per descendant. Fed as words from registers: hashing a stored struct as bytes reads stores back across seams that can't be forwarded.
#[inline]
pub(super) fn build_cascade_prefix(parent: CascadeContext) -> Hasher {
    let (clip, clip_present) = match parent.clip {
        Some(rect) => (rect.canon_lanes(), true),
        None => ([0; 4], false),
    };
    let flags = u32::from(clip_present)
        | (u32::from(parent.disabled) << 1)
        | (u32::from(parent.invisible) << 2);
    let word = |lo: u32, hi: u32| u64::from(lo) | (u64::from(hi) << 32);
    let mut h = Hasher::new();
    h.write_u64(word(
        float_hash::canon_bits(parent.transform.translation.x),
        float_hash::canon_bits(parent.transform.translation.y),
    ));
    h.write_u64(word(
        float_hash::canon_bits(parent.transform.scale - 1.0),
        flags,
    ));
    h.write_u64(word(clip[0], clip[1]));
    h.write_u64(word(clip[2], clip[3]));
    h
}

#[inline]
pub(super) fn finish_cascade_input(
    prefix: &Hasher,
    layout_rect: Rect,
    invisible: bool,
) -> CascadeInputHash {
    let mut h = prefix.clone();
    layout_rect.hash_visual(&mut h);
    CascadeInputHash::pack(h.finish(), invisible)
}
