//! The cascade walk: [`CascadeEngine`] and the scratch it carries.
//!
//! Mirrors `layout::engine` — the module root holds the retained
//! product, this file holds the machinery that fills it.

use crate::common::hash::Hasher;
use crate::display::Display;
use crate::input::key_class::KeyFilter;
use crate::input::sense::Sense;
use crate::layout::types::placement::Placement;
use crate::layout::{LayerLayout, Layout};
use crate::primitives::approx;
use crate::primitives::approx::FloatHash;
use crate::primitives::rect::Rect;
use crate::primitives::span::Span;
use crate::primitives::translate_scale::TranslateScale;
use crate::scene::cascade::counters::CascadeCounters;
use crate::scene::cascade::entry::{EntryRow, HitRow, ScopeRow};
use crate::scene::cascade::paint::PaintArena;
use crate::scene::cascade::paint_rect::{PaintRectCtx, compute_paint_rect};
use crate::scene::cascade::{Cascade, CascadeInputHash, LayerCascade};
use crate::scene::forest::Forest;
use crate::scene::layer::Layer;
use crate::scene::tree::Tree;
use crate::scene::tree::record::NodeId;
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
    layer: Layer,
}

#[derive(Debug)]
struct Frame {
    transform: TranslateScale,
    clip: Option<Rect>,
    disabled: bool,
    invisible: bool,
    subtree_end: u32,
    /// Node index this frame represents — used to write back
    /// `subtree_paint_rect` into `Cascade::subtree_paint_rects` when
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
    /// `None` on an incremental frame: the retained `cascade_input`s
    /// stay valid there, so no prefix is folded and none is read.
    cascade_prefix: Option<Hasher>,
}

#[derive(Debug, Default)]
pub(crate) struct CascadeEngine {
    stack: Vec<Frame>,
    paint_scratch: PaintArena,
    display_scale: Option<f32>,
    /// Test/bench observability for this pass — see [`CascadeCounters`].
    pub(crate) counters: CascadeCounters,
}

impl CascadeEngine {
    /// Update the frozen cascade result. Stable subtrees are retained
    /// in place; a paint-row cardinality or tree-size change falls
    /// back to a complete rebuild.
    #[profiling::function]
    pub(crate) fn run(
        &mut self,
        forest: &Forest,
        layout: &Layout,
        display: Display,
        cascade: &mut Cascade,
    ) {
        if !self.can_update(forest, layout, display, cascade) {
            self.run_full(forest, layout, display, cascade);
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
                display.scale_factor,
            );
            if !incremental_complete {
                self.counters.abandoned_incremental();
                self.run_full(forest, layout, display, cascade);
                return;
            }
        }
    }

    /// A match proves every retained non-paint cascade and hit-test
    /// column remains valid; the incremental walk only repairs paint.
    fn can_update(
        &self,
        forest: &Forest,
        layout: &Layout,
        display: Display,
        cascade: &Cascade,
    ) -> bool {
        if self.display_scale != Some(display.scale_factor) {
            return false;
        }
        let total = forest.total_nodes();
        if cascade.entries.len() != total {
            return false;
        }
        let mut entries_base = 0u32;
        for (layer, tree) in forest.trees.iter_paint_order() {
            let n = tree.records.len();
            let lc = &cascade.layers[layer];
            if lc.entries_base != entries_base
                || lc.static_hash != tree.fingerprint.cascade_static
                || lc.paint_cardinality != tree.fingerprint.paint_cardinality
                || lc.subtree_hashes.len() != n
            {
                return false;
            }
            if lc.layout_hash != layout[layer].rect_hash() {
                return false;
            }
            // No structural walk here: `cascade_static` folds each
            // node's `subtree_end`, so the scalar compare above already
            // covers nesting. This used to zip the whole column every
            // run — the one walk standing between `Cascade::subtree_ends`
            // and the sparse ancestry column it is documented to be.
            entries_base += n as u32;
        }
        true
    }

    pub(crate) fn run_full(
        &mut self,
        forest: &Forest,
        layout: &Layout,
        display: Display,
        cascade: &mut Cascade,
    ) {
        self.counters.full_rebuild();
        let total = forest.total_nodes();
        cascade.entries.clear();
        cascade.entries.reserve_exact(total);
        cascade.hits.clear();
        cascade.scopes.clear();

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
                    layer,
                }),
                display.scale_factor,
            );
            debug_assert!(full_complete);
            cascade.layers[layer]
                .subtree_hashes
                .copy_from_slice(&tree.rollups.subtree);
            debug_assert_eq!(
                cascade.entries.len() as u32 - entries_base,
                n as u32,
                "run_tree must emit one entry per recorded node",
            );
            cascade.layers[layer].static_hash = tree.fingerprint.cascade_static;
            cascade.layers[layer].paint_cardinality = tree.fingerprint.paint_cardinality;
            cascade.layers[layer].layout_hash = layout[layer].rect_hash();
        }

        // `SeenIds::pre_record` clears `curr` before a relayout pass can
        // query the preceding pass's responses.
        cascade.by_id.clone_from(&forest.ids.curr);
        self.display_scale = Some(display.scale_factor);
    }
}

/// Fingerprint of everything [`CascadeEngine::run`] reads, cheaply.
/// Equal fingerprints across two frames ⇒ identical cascade output, so
/// `FrameCycle::post_record` skips the run and reuses last frame's `Cascade`
/// (O5 stage 0 — full-frame skip, gated on the frame runtime's cascade fingerprint).
/// Folds:
/// - the exact surface (a sub-quantum resize can hit the measure
///   cache yet still re-arrange, so the *exact* rect must be here);
/// - every root's `subtree_hash`, which already captures all cascade
///   authoring — transforms (`PanelExtras`), clip/disabled/focusable
///   (`attrs`), visibility, shapes, chrome;
///
/// Lives here, beside the walk it mirrors, on purpose: the skip is
/// only sound while this enumeration covers every input `run_tree`
/// (and the arrange pass feeding it) consumes. Adding a cascade input
/// without folding it here silently reuses stale cascade — keep the
/// two in one review's field of view.
pub(crate) fn cascade_fingerprint(forest: &Forest, display: Display) -> u64 {
    let mut h = Hasher::new();
    h.write_u32(display.physical.x);
    h.write_u32(display.physical.y);
    display.scale_factor.hash_eq(&mut h);
    for (layer, tree) in forest.trees.iter_paint_order() {
        // Layer discriminant: an identical root subtree migrating
        // between side layers (Popup → Tooltip) must not alias, or
        // the skip reuses per-layer columns sized for the old
        // assignment and the damage pass indexes them out of
        // bounds.
        h.write_u8(layer as u8);
        for slot in &tree.roots {
            // A root's own id does not reach the subtree hash used by
            // this fingerprint — `compute_rollups` folds only child ids
            // into parents — so include it directly.
            h.write_u64(tree.records.widget_id()[slot.first_node.idx()].0);
            h.write_u64(tree.rollups.subtree[slot.first_node.idx()].0);
            // Placement lives outside node hashes but changes arranged rects.
            match slot.placement {
                Placement::Fixed { anchor, size } => {
                    h.write_u8(0);
                    anchor.hash_visual(&mut h);
                    match size {
                        Some(size) => {
                            h.write_u8(1);
                            size.hash_visual(&mut h);
                        }
                        None => h.write_u8(0),
                    }
                }
                Placement::Overlay(position) => {
                    h.write_u8(1);
                    position.anchor.hash_visual(&mut h);
                    h.write_u8(position.side as u8);
                    h.write_u8(position.align as u8);
                    position.gap.hash_visual(&mut h);
                }
            }
        }
    }
    h.finish()
}

/// Finalize one stack frame: write the rolled-up
/// `subtree_paint_rect` into the parallel `subtree_paint_rects` slot
/// for the frame's node, then union upward into the now-top frame so
/// the rollup ripples to the root. Called from both the per-node
/// pop loop and the end-of-tree drain — identical logic, one source.
#[inline]
fn finalize_frame(stack: &mut [Frame], subtree_paint_rects: &mut [Rect], popped: Frame) {
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
        // `None` while repairing: the retained `cascade_input`s stay
        // valid, so nothing folds a prefix and nothing reads one.
        let root_prefix = (!INCREMENTAL)
            .then(|| build_cascade_prefix(TranslateScale::IDENTITY, None, false, false));

        let mut i: u32 = 0;
        while i < n {
            // Pop completed frames, rolling each up into its parent.
            while let Some(top) = self.stack.last() {
                if i < top.subtree_end {
                    break;
                }
                let popped = self.stack.pop().unwrap();
                finalize_frame(&mut self.stack, &mut lc.subtree_paint_rects, popped);
            }
            let (parent_transform, parent_clip, parent_dis, parent_inv, parent_prefix) =
                match self.stack.last() {
                    Some(p) => (
                        p.transform,
                        p.clip,
                        p.disabled,
                        p.invisible,
                        p.cascade_prefix.as_ref(),
                    ),
                    None => (
                        TranslateScale::IDENTITY,
                        None,
                        false,
                        false,
                        root_prefix.as_ref(),
                    ),
                };

            let iu = i as usize;
            let id = NodeId(i);
            let attrs = attrs_col[iu];
            let layout_core = layout_col[iu];

            let disabled = parent_dis || attrs.is_disabled();
            let owner_visible = layout_core.meta.visibility().is_visible();
            let invisible = parent_inv || !owner_visible;

            let layout_rect = layout.rect[iu];
            // `.end()` strips the packed grid flag — downstream uses (walk
            // cursor, leaf compare) need the clean pre-order end.
            let subtree_end = ends[iu].end();
            let has_children = subtree_end != i + 1;
            if INCREMENTAL && lc.subtree_hashes[iu] == subtree_hashes[iu] {
                if let Some(parent) = self.stack.last_mut() {
                    parent.subtree_paint_rect =
                        parent.subtree_paint_rect.union(lc.subtree_paint_rects[iu]);
                }
                i = subtree_end;
                continue;
            }

            let screen_rect = parent_transform.apply_rect(layout_rect);
            let visible_rect = parent_clip.map_or(screen_rect, |c| screen_rect.clamp_to(c));
            // The transform descendants inherit *and* direct shapes paint
            // under (the `Panel::transform` contract): `parent ∘
            // self_anchored`. Computed once here — `transform_of` is a
            // sparse-column probe and `compose` is 3×mul+3×add, so the
            // `None` arm (most nodes have no transform) skips the compose
            // entirely, the steady-state path. `compute_paint_rect` reuses
            // this as its `shape_transform` rather than recomposing.
            //
            // Scale pivots about the node's own `layout_rect.min`, not the
            // cascade's (0, 0); `anchored_at` cancels the
            // `panel.min * (1 - scale)` drift a raw compose against
            // absolute-coord layout rects would introduce (identity-
            // preserving — no-op at `scale == 1`). See
            // `TranslateScale::anchored_at`.
            let node_transform = tree.transform_of(id);
            let desc_transform = match node_transform {
                Some(t) => parent_transform.compose(t.anchored_at(layout_rect.min)),
                None => parent_transform,
            };
            let clips = attrs.clip_mode().is_clip();
            // Encoder's clip mask is `rect.deflated_by(padding)`, pushed
            // **before** the body. Direct shapes and descendants both
            // paint inside it. Mirror that here so per-shape damage rects
            // and inherited child clips reflect what actually paints —
            // otherwise a TextEdit's tall text shape (extent = full
            // shaped buffer) reports damage well past the editor's rect
            // on every scroll tick.
            let shape_clip = if clips {
                let mask_local = layout_rect.deflated_by(layout_core.padding);
                let mask_screen = parent_transform.apply_rect(mask_local);
                Some(parent_clip.map_or(mask_screen, |c| mask_screen.clamp_to(c)))
            } else {
                parent_clip
            };
            let ctx = PaintRectCtx {
                tree,
                layout,
                node: id,
                visible_rect,
                parent_transform,
                parent_clip,
                shape_clip,
                shape_transform: desc_transform,
                display_scale,
                clips,
                has_children,
            };
            let paint_rect = if INCREMENTAL {
                let old_span = lc.paint_arena.node_spans[iu];
                let paint_rect = compute_node_paint(ctx, owner_visible, &mut self.paint_scratch);
                let new_span = self.paint_scratch.node_spans[iu];
                if old_span.len != new_span.len {
                    return false;
                }
                lc.paint_arena.rows[old_span.range()]
                    .copy_from_slice(&self.paint_scratch.rows[new_span.range()]);
                paint_rect
            } else {
                compute_node_paint(ctx, owner_visible, &mut lc.paint_arena)
            };
            // Invisible nodes never paint, so seeding their subtree
            // rollup with `Rect::ZERO` keeps a long-lived hidden subtree
            // from inflating the ancestor's `subtree_paint_rect` (and
            // killing the encoder's viewport / damage cull at that
            // ancestor). Visibility is in `cascade_input` regardless, so
            // damage tracking is unaffected.
            let subtree_seed = if invisible { Rect::ZERO } else { paint_rect };
            if INCREMENTAL {
                lc.subtree_hashes[iu] = subtree_hashes[iu];
            } else {
                let parent_prefix =
                    parent_prefix.expect("a full rebuild always carries a cascade prefix");
                lc.cascade_inputs[iu] = finish_cascade_input(parent_prefix, layout_rect, invisible);
                lc.subtree_ends[iu] = subtree_end;
            }
            lc.subtree_paint_rects[iu] = subtree_seed;

            // Descendants inherit the deflated-mask clip — same value the
            // direct shapes were clipped to above and the encoder pushes
            // before the body.
            let desc_clip = shape_clip;
            if !INCREMENTAL {
                let sink = sink
                    .as_mut()
                    .expect("a full rebuild always passes its sink");
                let layer = sink.layer;
                let cascaded_off = disabled || invisible;
                let sense = if cascaded_off {
                    Sense::NONE
                } else {
                    attrs.sense()
                };
                let focusable = !cascaded_off && attrs.is_focusable();
                if sense != Sense::NONE || focusable {
                    sink.hits.push(HitRow {
                        rect: visible_rect,
                        widget_id: widget_ids[iu],
                        sense,
                        focusable,
                    });
                }
                // A scope in a disabled or invisible subtree owns
                // nothing, the same rule that nulls `sense` above.
                let filter = if cascaded_off {
                    KeyFilter::empty()
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
                    transform: parent_transform,
                    disabled,
                });
            }

            if !has_children {
                // Leaf: no descendants, so no frame — its
                // `subtree_paint_rects` slot already holds the seed written
                // above; fold the seed straight into the parent accumulator
                // (a non-painting leaf's `Rect::ZERO` seed is `union`'s
                // identity). Skips a per-leaf Frame push/pop and the 32 B
                // full-rebuild prefix-hash work leaves could never hand to
                // a child.
                if let Some(parent) = self.stack.last_mut() {
                    parent.subtree_paint_rect = parent.subtree_paint_rect.union(subtree_seed);
                }
            } else {
                self.stack.push(Frame {
                    transform: desc_transform,
                    clip: desc_clip,
                    disabled,
                    invisible,
                    subtree_end,
                    node_idx: iu,
                    subtree_paint_rect: subtree_seed,
                    cascade_prefix: (!INCREMENTAL).then(|| {
                        build_cascade_prefix(desc_transform, shape_clip, disabled, invisible)
                    }),
                });
            }
            i += 1;
        }
        // Drain frames whose subtree extends to the end of the tree —
        // they never hit the `< top.subtree_end` exit at the loop head.
        while let Some(popped) = self.stack.pop() {
            finalize_frame(&mut self.stack, &mut lc.subtree_paint_rects, popped);
        }
        true
    }
}

#[inline]
fn compute_node_paint(ctx: PaintRectCtx<'_>, owner_visible: bool, arena: &mut PaintArena) -> Rect {
    if !owner_visible {
        arena.node_spans[ctx.node.idx()] = Span::new(arena.rows.len() as u32, 0);
        return Rect::ZERO;
    }
    compute_paint_rect(ctx, arena)
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

#[inline]
pub(super) fn build_cascade_prefix(
    parent_transform: TranslateScale,
    parent_clip: Option<Rect>,
    parent_dis: bool,
    parent_inv: bool,
) -> Hasher {
    let (clip, clip_present) = match parent_clip {
        Some(rect) => (
            [
                approx::canon_bits(rect.min.x),
                approx::canon_bits(rect.min.y),
                approx::canon_bits(rect.size.w),
                approx::canon_bits(rect.size.h),
            ],
            true,
        ),
        None => ([0; 4], false),
    };
    let flags = (clip_present as u32) | ((parent_dis as u32) << 1) | ((parent_inv as u32) << 2);
    let packed = CascadePrefixBits {
        transform: [
            approx::canon_bits(parent_transform.translation.x),
            approx::canon_bits(parent_transform.translation.y),
            approx::canon_bits(parent_transform.scale - 1.0),
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
