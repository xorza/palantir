//! The cascade walk: [`CascadeEngine`] and the scratch it carries.
//!
//! Mirrors `layout::engine` — the module root holds the retained
//! product, this file holds the machinery that fills it.

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

/// The three tables one tree's walk appends to, plus the layer it is
/// walking — bundled because they are pushed to together and travel as
/// one, and because threading four more parameters through
/// `run_tree` is what the argument count is for.
///
/// The **full rebuild's** alone. An incremental walk repairs paint and
/// appends to none of them, which is why `run_tree` takes this as an
/// `Option` and that path passes `None` rather than a sink it will
/// never touch.
#[derive(Debug)]
struct TreeSink<'a> {
    entries: &'a mut Vec<EntryRow>,
    hits: &'a mut Vec<HitRow>,
    scopes: &'a mut Vec<ScopeRow>,
    tab_stops: &'a mut Vec<TabStopRow>,
    roots: &'a mut Vec<RootRow>,
    arrow_groups: &'a mut Vec<ArrowGroupRow>,
    layer: Layer,
    /// The root the walk is under, set as each one opens.
    root: WidgetId,
}

/// The four values a node hands its descendants, and the only inputs
/// [`build_cascade_prefix`] hashes. Bundled because they travel together
/// everywhere — down the stack, into the prefix, out of a frame — and two
/// of the four are adjacent `bool`s that swap silently.
#[derive(Clone, Copy, Debug)]
pub(super) struct CascadeContext {
    pub(super) transform: TranslateScale,
    pub(super) clip: Option<Rect>,
    pub(super) disabled: bool,
    pub(super) invisible: bool,
}

impl CascadeContext {
    /// What a layer root inherits: no transform, no clip, enabled, visible.
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
    /// Node index this frame represents — used to write back
    /// `subtree_paint_rect` into `LayerCascade::subtree_paint_rects` when
    /// this frame is popped (its subtree has been fully visited).
    node_idx: usize,
    /// Running union of this node's own `paint_rect` and the
    /// `subtree_paint_rect` of every descendant whose subtree has
    /// already been folded in. Each pop unions this into the new
    /// top frame so the rollup ripples upward to the root.
    subtree_paint_rect: Rect,
    /// Full-rebuild FxHasher state pre-populated with this frame's
    /// ancestor-derived hash inputs (transform / clip / disabled /
    /// invisible). Cloned once per descendant to seed `cascade_input` —
    /// descendants only fold in their own `layout_rect`, avoiding a
    /// re-hash of the 32 B ancestor prefix per node.
    ///
    /// Empty and unread on an incremental frame: the retained
    /// `cascade_input`s stay valid while repairing, so nothing folds it.
    /// A fresh [`Hasher`] is one `u64`, cheaper to carry than an `Option`
    /// saying it is unread — which every node of a *rebuild* would then
    /// have to unwrap.
    cascade_prefix: Hasher,
}

#[derive(Debug, Default)]
pub(crate) struct CascadeEngine {
    stack: Vec<Frame>,
    paint_scratch: PaintArena,
    /// Test/bench observability for this pass — see [`CascadeCounters`].
    pub(crate) counters: CascadeCounters,
}

impl CascadeEngine {
    /// Bring the frozen cascade result up to `key`. An unchanged key
    /// skips the run, a paint-only change repairs paint rows in place,
    /// and anything else rebuilds.
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
        if !self.can_update(forest, key, cascade) {
            self.run_full(forest, layout, display, key, cascade);
            return;
        }

        for (layer, tree) in forest.trees.iter_paint_order() {
            let n = tree.records.len();
            self.stack.clear();
            self.paint_scratch.reset_for(n);
            // No sink: this walk repairs paint and appends to none of
            // the three tables.
            let incremental_complete = self.run_tree::<true>(
                tree,
                &layout[layer],
                &mut cascade.layers[layer],
                None,
                display.scale_factor(),
            );
            if !incremental_complete {
                self.counters.abandoned_incremental();
                // Restarts at layer zero, over the layers this loop
                // already repaired: `run_full` rebuilds one flat `entries`
                // table whose per-layer bases it assigns as it walks, so
                // there is no entry point part-way through. The repeat
                // costs one extra paint repair, on a frame already
                // committed to a full rebuild.
                self.run_full(forest, layout, display, key, cascade);
                return;
            }
        }
        cascade.key = Some(*key);
    }

    /// Whether every retained non-paint cascade and hit-test column
    /// remains valid; the incremental walk only repairs paint.
    fn can_update(&self, forest: &Forest, key: &CascadeKey, cascade: &Cascade) -> bool {
        let Some(built) = &cascade.key else {
            return false;
        };
        if !built.differs_only_in_paint(key) {
            return false;
        }
        // The bases are a prefix sum over the per-layer counts, and the
        // equal structure hashes fold each layer's nesting, so these
        // only restate what the key already proved.
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
            let full_complete = self.run_tree::<false>(
                tree,
                &layout[layer],
                &mut cascade.layers[layer],
                Some(&mut TreeSink {
                    entries: &mut cascade.entries,
                    hits: &mut cascade.hits,
                    scopes: &mut cascade.scopes,
                    tab_stops: &mut cascade.tab_stops,
                    roots: &mut cascade.roots,
                    arrow_groups: &mut cascade.arrow_groups,
                    layer,
                    root: WidgetId::default(),
                }),
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

        // `SeenIds::pre_record` clears `curr` before a relayout pass can
        // query the preceding pass's responses.
        //
        // Refilled rather than `clone_from`: `curr` and `prev` swap every
        // frame, so the source alternates between two tables, and
        // hashbrown's `clone_from` reallocates whenever their bucket
        // counts differ — one widget-count spike would grow one of them
        // for good and make every later full rebuild free and allocate.
        // `clear` keeps this table's own capacity.
        cascade.by_id.clear();
        cascade.by_id.extend(&forest.ids.curr);
        cascade.key = Some(*key);
    }
}

/// Finalize one stack frame: write the rolled-up
/// `subtree_paint_rect` into the parallel `subtree_paint_rects` slot
/// for the frame's node, then union upward into the now-top frame so
/// the rollup ripples to the root. Called from both the per-node
/// pop loop and the end-of-tree drain — identical logic, one source.
#[inline]
const fn finalize_frame(stack: &mut [Frame], subtree_paint_rects: &mut [Rect], popped: &Frame) {
    subtree_paint_rects[popped.node_idx] = popped.subtree_paint_rect;
    if let Some(parent) = stack.last_mut() {
        // A subtree that paints nothing carries the `Rect::ZERO` seed;
        // `union` treats it as identity, so it can't anchor the
        // ancestor rollup at the origin.
        parent.subtree_paint_rect = parent.subtree_paint_rect.union(popped.subtree_paint_rect);
    }
}

impl CascadeEngine {
    /// Walk one tree, writing the cascade columns for it.
    ///
    /// Returns whether the walk *completed*. Only the incremental path
    /// can answer `false` — it bails when a node's repaired paint span
    /// changes length, because the retained arena can no longer be
    /// patched in place and the caller has to fall back to a full
    /// rebuild. A full rebuild writes every column from scratch and has
    /// nothing to bail on, so its caller asserts the `true`.
    ///
    /// `INCREMENTAL` is a const parameter so each path folds away the
    /// other's branches. Measured: that buys no time on `cascade/run`,
    /// and costs ~3.4 KB of codegen by inlining the walk into both
    /// callers — it is kept for the dead-code elimination that keeps
    /// each path's reads honest, not for speed.
    fn run_tree<const INCREMENTAL: bool>(
        &mut self,
        tree: &Tree,
        layout: &LayerLayout,
        lc: &mut LayerCascade,
        mut sink: Option<&mut TreeSink<'_>>,
        display_scale: f32,
    ) -> bool {
        let n = tree.records.len() as u32;
        let layout_col = tree.records.layout();
        let attrs_col = tree.records.attrs();
        let widget_ids = tree.records.widget_id();
        let ends = tree.records.subtree_end();
        let subtree_hashes = tree.rollups.subtree.as_slice();
        debug_assert_eq!(
            !INCREMENTAL,
            sink.is_some(),
            "the sink is the full rebuild's, and only its",
        );
        let root_prefix = frame_prefix::<INCREMENTAL>(CascadeContext::ROOT);

        let mut i: u32 = 0;
        while i < n {
            // Pop completed frames, rolling each up into its parent.
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
            // `.end()` strips the packed grid flag — downstream uses (walk
            // cursor, leaf compare) need the clean pre-order end.
            let subtree_end = ends[iu].end();
            let has_children = ends[iu].has_children(iu);
            if INCREMENTAL && lc.arena_hashes[iu] == subtree_hashes[iu] {
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
            // The transform descendants inherit *and* direct shapes paint
            // under (the `Panel::transform` contract): `parent ∘
            // self_anchored`. Computed once here — the probe is sparse and
            // `compose` is 3×mul+3×add, so the `None` arm (most nodes have
            // no transform) skips the compose entirely, the steady-state
            // path. `compute_paint_rect` reuses this as its
            // `shape_transform` rather than recomposing.
            let desc_transform = match tree.anchored_transform(id, layout_rect) {
                Some(t) => parent.transform.compose(t),
                None => parent.transform,
            };
            let clips = attrs.clip_mode().is_clip();
            // The encoder pushes the same inner box as the clip mask,
            // **before** the body, so direct shapes and descendants both
            // paint inside it. Clipping to it here is what makes per-shape
            // damage rects and inherited child clips reflect what actually
            // paints — otherwise a TextEdit's tall text shape (extent =
            // full shaped buffer) reports damage well past the editor's
            // rect on every scroll tick.
            let shape_clip = if clips {
                let mask_screen = parent
                    .transform
                    .apply_rect(layout_core.inner_rect(layout_rect));
                Some(paint_rect::clip_screen(mask_screen, parent.clip))
            } else {
                parent.clip
            };
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
            let paint_rect = if INCREMENTAL {
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
            // Invisible nodes never paint, so seeding their subtree
            // rollup with `Rect::ZERO` keeps a long-lived hidden subtree
            // from inflating the ancestor's `subtree_paint_rect` (and
            // killing the encoder's viewport / damage cull at that
            // ancestor). Visibility is in `cascade_input` regardless, so
            // damage tracking is unaffected.
            let subtree_seed = if invisible { Rect::ZERO } else { paint_rect };
            if INCREMENTAL {
                lc.arena_hashes[iu] = subtree_hashes[iu];
            } else {
                lc.cascade_inputs[iu] = finish_cascade_input(parent_prefix, layout_rect, invisible);
                lc.subtree_ends[iu] = subtree_end;
            }
            lc.subtree_paint_rects[iu] = subtree_seed;

            // Descendants inherit the deflated-mask clip — same value the
            // direct shapes were clipped to above and the encoder pushes
            // before the body.
            let desc_clip = shape_clip;
            // The `Option` is the const generic's decision made once, at
            // the call: a repair passes none. Reading it as one folds the
            // branch away in both monomorphizations, where testing
            // `INCREMENTAL` and then unwrapping asked the same question
            // twice and left a panic path on every node of a rebuild.
            if let Some(sink) = sink.as_mut() {
                let layer = sink.layer;
                if self.stack.is_empty() {
                    sink.root = widget_ids[iu];
                    sink.roots.push(RootRow {
                        layer,
                        id: sink.root,
                    });
                }
                // Names the keyboard half alone: the pointer half below
                // keeps routing to a disabled row, and only `invisible`
                // takes a node out of it.
                let keyboard_off = disabled || invisible;
                // Disabling takes away what a widget may *do*, never what
                // it occupies: it is still drawn, still under the
                // pointer, and still in the way of everything it covers,
                // so it keeps the sense it declared and goes on being
                // routed to. Nothing comes back, because the response it
                // reads has its interaction half cleared. Invisible is
                // the other ruling — what is not drawn takes nothing.
                let sense = if invisible {
                    Sense::NONE
                } else {
                    attrs.sense()
                };
                // Focus is the exception: routing a press to a disabled
                // widget absorbs it, but *focusing* one would put the
                // keyboard somewhere that answers no key.
                let focusable = !keyboard_off && attrs.is_focusable();
                if !keyboard_off && let Some(axis) = attrs.arrow_focus() {
                    sink.arrow_groups.push(ArrowGroupRow {
                        id: widget_ids[iu],
                        axis,
                    });
                }
                if focusable && attrs.is_tab_stop() {
                    sink.tab_stops.push(TabStopRow {
                        layer,
                        root: sink.root,
                        id: widget_ids[iu],
                        index: tree.bounds(id).tab_index,
                    });
                }
                if sense != Sense::NONE || focusable {
                    sink.hits.push(HitRow {
                        rect: visible_rect,
                        widget_id: widget_ids[iu],
                        sense,
                        focusable,
                        disabled,
                    });
                }
                // A scope in a disabled or invisible subtree owns
                // nothing, the same rule `focusable` follows: a key has
                // nowhere to go there.
                let filter = if keyboard_off {
                    KeyFilter::NONE
                } else {
                    attrs.key_filter()
                };
                if filter.is_scope() {
                    sink.scopes.push(ScopeRow {
                        layer,
                        id: widget_ids[iu],
                        filter,
                    });
                }
                sink.entries.push(EntryRow {
                    rect: visible_rect,
                    transform: parent.transform,
                    disabled,
                });
            }

            if has_children {
                let cascade = CascadeContext {
                    transform: desc_transform,
                    clip: desc_clip,
                    disabled,
                    invisible,
                };
                self.stack.push(Frame {
                    cascade,
                    subtree_end,
                    node_idx: iu,
                    subtree_paint_rect: subtree_seed,
                    cascade_prefix: frame_prefix::<INCREMENTAL>(cascade),
                });
            } else {
                // Leaf: no descendants, so no frame — its
                // `subtree_paint_rects` slot already holds the seed written
                // above; fold the seed straight into the parent accumulator
                // (a non-painting leaf's `Rect::ZERO` seed is `union`'s
                // identity). Skips a per-leaf Frame push/pop and the 32 B
                // full-rebuild prefix-hash work leaves could never hand to
                // a child.
                if let Some(parent_frame) = self.stack.last_mut() {
                    parent_frame.subtree_paint_rect =
                        parent_frame.subtree_paint_rect.union(subtree_seed);
                }
            }
            i += 1;
        }
        // Drain frames whose subtree extends to the end of the tree —
        // they never hit the `< top.subtree_end` exit at the loop head.
        while let Some(popped) = self.stack.pop() {
            finalize_frame(&mut self.stack, &mut lc.subtree_paint_rects, &popped);
        }
        true
    }
}

/// The node's paint rows and extent, or an empty span when nothing in
/// this node can reach the surface.
///
/// The flag is the cascaded reading, not the node's own: a subtree under
/// a non-`Visible` ancestor paints nothing, so computing its extents
/// buys rows no pass ever reads — the encoder stops at the ancestor, and
/// `subtree_paint_rect` seeds such a subtree at zero either way. Damage
/// reads the emptied span as a rows→rowless transition and clears the
/// pixels the subtree held, which is the same answer it reached before
/// through every descendant's `cascade_input` flipping at once.
///
/// `Tree::container_text` drops a subtree on the same reading, so the
/// two walks agree on which nodes own shaped runs — the pairing
/// [`TextRuns`](crate::layout::text::text_runs::TextRuns) makes between a text
/// record and its `ShapedText` depends on that.
#[inline]
fn compute_node_paint(ctx: PaintRectCtx<'_>, invisible: bool, arena: &mut PaintArena) -> Rect {
    if invisible {
        arena.node_spans[ctx.node.idx()] = Span::new(arena.rows.len() as u32, 0);
        return Rect::ZERO;
    }
    paint_rect::compute_paint_rect(ctx, arena)
}

/// Ancestor-derived portion of the `cascade_input` hash — folded once
/// per stack frame at push time (32 B) and cloned per descendant. Split
/// out from the per-node suffix (`layout_rect`) so a tree-shaped UI
/// avoids re-hashing the parent context on every node.
#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::NoUninit)]
pub(super) struct CascadePrefixBits {
    transform: [u32; 4],
    clip: [u32; 4],
}

/// The prefix a [`Frame`] carries, folded only where one is read.
///
/// An incremental repair reads no prefix — its `cascade_input`s are
/// retained — so it takes the empty hasher rather than paying the fold.
#[inline]
fn frame_prefix<const INCREMENTAL: bool>(cascade: CascadeContext) -> Hasher {
    if INCREMENTAL {
        Hasher::new()
    } else {
        build_cascade_prefix(cascade)
    }
}

#[inline]
pub(super) fn build_cascade_prefix(parent: CascadeContext) -> Hasher {
    let (clip, clip_present) = match parent.clip {
        Some(rect) => (rect.canon_lanes(), true),
        None => ([0; 4], false),
    };
    let flags = u32::from(clip_present)
        | (u32::from(parent.disabled) << 1)
        | (u32::from(parent.invisible) << 2);
    let packed = CascadePrefixBits {
        transform: [
            float_hash::canon_bits(parent.transform.translation.x),
            float_hash::canon_bits(parent.transform.translation.y),
            float_hash::canon_bits(parent.transform.scale - 1.0),
            flags,
        ],
        clip,
    };
    let mut h = Hasher::new();
    h.pod(&packed);
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
