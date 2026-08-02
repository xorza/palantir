//! The cascade walk: [`CascadeEngine`] and the scratch it carries.
//!
//! Mirrors `layout::engine` — the module root holds the retained
//! product, this file holds the machinery that fills it.

use crate::common::content_hash::ContentHash;
use crate::common::hash::Hasher;
use crate::display::Display;
use crate::input::key_class::KeyFilter;
use crate::input::sense::Sense;
use crate::layout::{LayerLayout, Layout};
use crate::primitives::approx;
use crate::primitives::rect::Rect;
use crate::primitives::size::Size;
use crate::primitives::span::Span;
use crate::primitives::transform::TranslateScale;
use crate::scene::cascade::entry::{EntryRow, HitRow, ScopeRow};
use crate::scene::cascade::paint::{Paint, PaintArena};
use crate::scene::cascade::{Cascade, CascadeInputHash, LayerCascade};
use crate::scene::forest::Forest;
use crate::scene::layer::Layer;
use crate::scene::shapes::record::{ShapeRecord, shadow_paint_rect_local, text_paint_bbox_local};
use crate::scene::tree::Tree;
use crate::scene::tree::iter::{TreeItem, TreeItems};
use crate::scene::tree::record::NodeId;
use crate::scene::tree::recording::Placement;
use crate::shape::stroke_bounds::{HALF_FRINGE, stroked_bbox};
use crate::text::TEXT_SCALE_STEP;
use glam::Vec2;
use std::hash::Hasher as _;

/// The three tables one tree's walk appends to, plus the layer it is
/// walking — bundled because they are pushed to together and travel as
/// one, and because threading four more parameters through
/// `run_tree` is what the argument count is for.
struct TreeSink<'a> {
    entries: &'a mut Vec<EntryRow>,
    hits: &'a mut Vec<HitRow>,
    scopes: &'a mut Vec<ScopeRow>,
    layer: Layer,
}

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
    /// re-hash of the 32 B ancestor prefix per node. Incremental frames
    /// carry an empty hasher because their retained inputs stay valid.
    cascade_prefix: Hasher,
}

impl std::fmt::Debug for Frame {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Frame")
            .field("transform", &self.transform)
            .field("clip", &self.clip)
            .field("disabled", &self.disabled)
            .field("invisible", &self.invisible)
            .field("subtree_end", &self.subtree_end)
            .field("node_idx", &self.node_idx)
            .field("subtree_paint_rect", &self.subtree_paint_rect)
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Default)]
pub(crate) struct CascadeEngine {
    stack: Vec<Frame>,
    paint_scratch: PaintArena,
    display_scale: Option<f32>,
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
            let incremental_complete = self.run_tree::<true>(
                tree,
                &layout.layers[layer],
                &mut cascade.layers[layer],
                &mut TreeSink {
                    entries: &mut cascade.entries,
                    hits: &mut cascade.hits,
                    scopes: &mut cascade.scopes,
                    layer,
                },
                display.scale_factor,
            );
            if !incremental_complete {
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
        let total: usize = forest.trees.0.iter().map(|tree| tree.records.len()).sum();
        if cascade.entries.len() != total {
            return false;
        }
        let mut entries_base = 0u32;
        for (layer, tree) in forest.trees.iter_paint_order() {
            let n = tree.records.len();
            let lc = &cascade.layers[layer];
            if lc.entries_base != entries_base
                || lc.static_hash != tree.rollups.cascade_static
                || lc.subtree_hashes.len() != n
            {
                return false;
            }
            if lc.layout_hash != layout.layers[layer].rect_hash() {
                return false;
            }
            if lc
                .subtree_ends
                .iter()
                .zip(tree.records.subtree_end())
                .any(|(&previous, current)| previous != current.end())
            {
                return false;
            }
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
        let total: usize = forest.trees.0.iter().map(|tree| tree.records.len()).sum();
        cascade.entries.clear();
        cascade.entries.reserve(total);
        cascade.hits.clear();
        cascade.scopes.clear();

        for (layer, tree) in forest.trees.iter_paint_order() {
            let n = tree.records.len();
            let entries_base = cascade.entries.len() as u32;
            cascade.layers[layer].reset_for(n, entries_base);
            self.stack.clear();
            let full_complete = self.run_tree::<false>(
                tree,
                &layout.layers[layer],
                &mut cascade.layers[layer],
                &mut TreeSink {
                    entries: &mut cascade.entries,
                    hits: &mut cascade.hits,
                    scopes: &mut cascade.scopes,
                    layer,
                },
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
            cascade.layers[layer].static_hash = tree.rollups.cascade_static;
            cascade.layers[layer].layout_hash = layout.layers[layer].rect_hash();
        }

        // `SeenIds::pre_record` clears `curr` before a relayout pass can
        // query the preceding pass's responses.
        cascade.by_id.clone_from(&forest.ids.curr);
        self.display_scale = Some(display.scale_factor);
    }
}

/// Fingerprint of everything [`CascadeEngine::run`] reads, cheaply.
/// Equal fingerprints across two frames ⇒ identical cascade output, so
/// `Ui::post_record` skips the run and reuses last frame's `Cascade`
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
    approx::hash_f32(display.scale_factor, &mut h);
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
                    approx::hash_visual_vec2(anchor, &mut h);
                    match size {
                        Some(size) => {
                            h.write_u8(1);
                            approx::hash_visual_size(size, &mut h);
                        }
                        None => h.write_u8(0),
                    }
                }
                Placement::Overlay(position) => {
                    h.write_u8(1);
                    approx::hash_visual_rect(position.anchor, &mut h);
                    h.write_u8(position.side as u8);
                    h.write_u8(position.align as u8);
                    approx::hash_visual_f32(position.gap, &mut h);
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
    // Compile-time specialization keeps the full rebuild free of incremental branches.
    fn run_tree<const INCREMENTAL: bool>(
        &mut self,
        tree: &Tree,
        layout: &LayerLayout,
        lc: &mut LayerCascade,
        sink: &mut TreeSink<'_>,
        display_scale: f32,
    ) -> bool {
        let TreeSink {
            entries,
            hits,
            scopes,
            layer,
        } = sink;
        let layer = *layer;
        let n = tree.records.len() as u32;
        let layout_col = tree.records.layout();
        let attrs_col = tree.records.attrs();
        let widget_ids = tree.records.widget_id();
        let ends = tree.records.subtree_end();
        let subtree_hashes = tree.rollups.subtree.as_slice();
        let root_prefix = if INCREMENTAL {
            Hasher::new()
        } else {
            build_cascade_prefix(TranslateScale::IDENTITY, None, false, false)
        };

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
                        &p.cascade_prefix,
                    ),
                    None => (TranslateScale::IDENTITY, None, false, false, &root_prefix),
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
            let visible_rect = parent_clip.map_or(screen_rect, |c| screen_rect.intersect(c));
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
                Some(parent_clip.map_or(mask_screen, |c| mask_screen.intersect(c)))
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
                lc.cascade_inputs[iu] = finish_cascade_input(parent_prefix, layout_rect, invisible);
                lc.subtree_ends[iu] = subtree_end;
            }
            lc.subtree_paint_rects[iu] = subtree_seed;

            // Descendants inherit the deflated-mask clip — same value the
            // direct shapes were clipped to above and the encoder pushes
            // before the body.
            let desc_clip = shape_clip;
            if !INCREMENTAL {
                let cascaded_off = disabled || invisible;
                let sense = if cascaded_off {
                    Sense::NONE
                } else {
                    attrs.sense()
                };
                let focusable = !cascaded_off && attrs.is_focusable();
                if sense != Sense::NONE || focusable {
                    hits.push(HitRow {
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
                    scopes.push(ScopeRow {
                        layer,
                        id: widget_ids[iu],
                        filter,
                    });
                }
                entries.push(EntryRow {
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
                    cascade_prefix: if INCREMENTAL {
                        Hasher::new()
                    } else {
                        build_cascade_prefix(desc_transform, shape_clip, disabled, invisible)
                    },
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
    approx::hash_visual_rect(layout_rect, &mut h);
    CascadeInputHash::pack(h.finish(), invisible)
}

/// Lift an owner-local rect into screen space: translate by the owner's
/// arranged origin, apply the relevant transform (`parent_transform`
/// for chrome / clip lift, `shape_transform` for shapes), then clip
/// to the ancestor clip. One source of truth for the three coord-
/// space hops the paint emit does.
#[inline]
fn lift_to_screen(local: Rect, origin: Vec2, t: TranslateScale, clip: Option<Rect>) -> Rect {
    let r = t.apply_rect(Rect {
        min: origin + local.min,
        size: local.size,
    });
    clip_screen(r, clip)
}

#[inline]
fn clip_screen(screen: Rect, clip: Option<Rect>) -> Rect {
    clip.map_or(screen, |c| screen.intersect(c))
}

/// Pad a text shape's screen rect by half a `TEXT_SCALE_STEP` of its
/// measured extent on each axis side, then re-clamp to `clip`.
///
/// The composer paints glyphs at the ladder-*snapped* scale
/// (`composer::snap_text_scale`), while the cascade lifts the rect at
/// the unsnapped scale. The painted block can be up to
/// `|snapped − cascade| ≤ STEP/2` longer per axis than the lifted
/// rect, which works out to `measured × STEP/2` of absolute screen
/// pixels per side — independent of cascade scale. A local-coord pad
/// would multiply by cascade and underflow at `cascade < 1`
/// (zoomed-out content), leaking glyph fringes past the damage rect.
/// Padding in screen space keeps damage covering the worst-case
/// painted extent at any zoom.
#[inline]
fn inflate_text_damage(screen: Rect, measured: Size, clip: Option<Rect>) -> Rect {
    // `screen` is already clipped, so a fully-off-clip run has collapsed
    // to zero on an axis (a zero-width box pinned at the clip edge). It
    // has no visible glyphs to pad; inflating it here would re-grow the
    // box *back across the clip edge*, fabricating a sub-pixel damage
    // sliver at the viewport edge for text that isn't on screen at all
    // (the "offscreen node casts a shadow at the window edge" bug). Leave
    // a non-paintable box empty — `is_paint_empty` also folds in the NaN
    // and float-boundary near-zero cases a bare `<= 0` compare would miss.
    if screen.is_paint_empty() {
        return screen;
    }
    let pad_w = measured.w * (TEXT_SCALE_STEP * 0.5);
    let pad_h = measured.h * (TEXT_SCALE_STEP * 0.5);
    let inflated = Rect {
        min: Vec2::new(screen.min.x - pad_w, screen.min.y - pad_h),
        size: Size {
            w: screen.size.w + 2.0 * pad_w,
            h: screen.size.h + 2.0 * pad_h,
        },
    };
    match clip {
        Some(c) => inflated.intersect(c),
        None => inflated,
    }
}

/// Push one paint row and fold its screen rect into the running union
/// in a single step. [`compute_paint_rect`]'s invariant requires the
/// union to track exactly the set of pushed non-paint-empty rows;
/// doing both here makes the two legs impossible to desync at a call
/// site. A paint-empty screen (shape fully clipped away) still pushes
/// its row — damage matches rows by identity and needs the slot — but
/// stays out of the union, which would otherwise grow to include the
/// degenerate box pinned at the clip edge.
#[inline]
fn push_paint(arena: &mut PaintArena, union: &mut Option<Rect>, screen: Rect, hash: ContentHash) {
    if !screen.is_paint_empty() {
        *union = Some(union.map_or(screen, |a| a.union(screen)));
    }
    arena.rows.push(Paint { screen, hash });
}

/// Inputs to [`compute_paint_rect`], threaded from `run_tree`.
///
/// Everything here is something the walk **already holds for its own
/// reasons**, so passing it is reuse rather than a wide-parameter habit;
/// a field earns its place by that test alone. `shape_transform` (the
/// `parent ∘ self_anchored` descendants also inherit) and `clips` are
/// the pointed cases — computed once at the call site so we don't
/// re-probe the sparse `transform_of` column, recompose the transform,
/// or re-read the SoA `attrs` column, all of which showed up as
/// duplicate work in cascade profiling. `visible_rect` is the same
/// bargain from the other end: the full walk pushes it into `hits` and
/// `entries` regardless, and deriving it here would apply and intersect
/// it a second time per node.
///
/// What is *not* here is the counterpart: `layout_rect` and the node's
/// `padding` are one indexed load each off lines this walk has already
/// touched, and `padding` is read only by the text arm — so they are
/// derived below instead of widening every node's bundle by 24 B.
struct PaintRectCtx<'a> {
    tree: &'a Tree,
    layout: &'a LayerLayout,
    node: NodeId,
    visible_rect: Rect,
    parent_transform: TranslateScale,
    parent_clip: Option<Rect>,
    shape_clip: Option<Rect>,
    shape_transform: TranslateScale,
    display_scale: f32,
    clips: bool,
    has_children: bool,
}

impl std::fmt::Debug for PaintRectCtx<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PaintRectCtx")
            .field("node", &self.node)
            .field("visible_rect", &self.visible_rect)
            .field("parent_transform", &self.parent_transform)
            .field("parent_clip", &self.parent_clip)
            .field("shape_clip", &self.shape_clip)
            .field("shape_transform", &self.shape_transform)
            .field("display_scale", &self.display_scale)
            .field("clips", &self.clips)
            .field("has_children", &self.has_children)
            .finish_non_exhaustive()
    }
}

/// Emit every paint row for `node` — chrome at row 0 when present,
/// then direct shapes and child markers in record order — write the
/// covering [`Span`] into `node_spans[node]`, and return the
/// screen-space union of the pixel-producing rows — used locally as
/// the `subtree_paint_rects` seed for the encoder's cull. Damage
/// recomputes the same union from the `paint_arena` rows on demand
/// (its cold paths), so it isn't stored per node.
///
/// Chrome rides `parent_transform` (encoder emits chrome before the
/// body push); shapes ride `shape_transform = parent ∘ self_anchored`
/// (inside the body push, per `Panel::transform`). Child markers are
/// pushed raw (zero screen, child `WidgetId` as hash) — they exist so
/// the damage diff sees the paint-order interleave; the child's pixels
/// are covered by its own rows.
///
/// # Invariant
///
/// The returned `Rect` is bit-identical to the screen-space union of
/// the non-paint-empty rows in
/// `arena.rows[paints_start..arena.rows.len()]` — the same union
/// `damage::union_screens` recomputes from the stored rows.
/// [`push_paint`] keeps the union and the pushed rows in lockstep;
/// child markers bypass it (zero rect, no pixels), and the chromeless
/// clip-only branch is the sole fold-without-push case (it contributes
/// a cull rect but emits no pixels).
fn compute_paint_rect(ctx: PaintRectCtx<'_>, arena: &mut PaintArena) -> Rect {
    let PaintRectCtx {
        tree,
        layout,
        node,
        visible_rect,
        parent_transform,
        parent_clip,
        shape_clip,
        shape_transform,
        display_scale,
        clips,
        has_children,
    } = ctx;
    // The walk read this same slot to build `visible_rect`, so it is a
    // hot line rather than a fresh fetch.
    let layout_rect = layout.rect[node.idx()];
    let paints_start = arena.rows.len() as u32;

    // `Option<Rect>` because zero-size sentinels bias `Rect::union`
    // toward the origin and an owner-rect seed would inflate damage
    // for chromeless shape hosts.
    let mut union: Option<Rect> = None;

    let owner_local = Rect {
        min: Vec2::ZERO,
        size: layout_rect.size,
    };

    if let Some(bg) = tree.chrome(node) {
        let screen = if bg.shadow.is_noop() {
            visible_rect
        } else {
            let g = bg.shadow.geom();
            let chrome_local = owner_local.union(shadow_paint_rect_local(
                None,
                layout_rect.size,
                g.offset,
                g.blur,
                g.spread,
                bg.shadow.inset(),
            ));
            lift_to_screen(chrome_local, layout_rect.min, parent_transform, parent_clip)
        };
        push_paint(arena, &mut union, screen, bg.hash);
    } else if clips {
        // Chromeless clip-only container: union the owner rect into
        // the cull rollup so the encoder emits the PushClip/PopClip
        // pair even when the subtree paints nothing (empty scroll
        // host, etc.). No Paint row — the node contributes no pixels.
        union = Some(visible_rect);
    }

    let has_shapes = tree.records.shape_span()[node.idx()].len > 0;
    if has_shapes || has_children {
        let text_span = layout.text_spans[node.idx()];
        let mut text_ord: u32 = 0;
        let shape_hashes = tree.shapes.hashes.as_slice();
        let widget_ids = tree.records.widget_id();
        for item in TreeItems::new(&tree.records, &tree.shapes.records, node) {
            let (idx, s) = match item {
                TreeItem::ShapeRecord(idx, s) => (idx, s),
                TreeItem::Child(c) => {
                    arena.rows.push(Paint {
                        screen: Rect::ZERO,
                        hash: ContentHash(widget_ids[c.id.idx()].0),
                    });
                    continue;
                }
            };
            // Every direct text shape has one layout-derived entry, whether
            // measure produced it for a leaf or post-arrange shaping produced
            // it for a container.
            let screen = match s {
                ShapeRecord::Text {
                    local_origin,
                    align,
                    ..
                } => {
                    debug_assert!(
                        text_ord < text_span.len,
                        "cascade saw a text shape without a matching ShapedText entry",
                    );
                    let shaped = layout.text_shapes[(text_span.start + text_ord) as usize];
                    text_ord += 1;
                    // Read here rather than carried in: the text arm is
                    // the only reader, so a node with no text shape never
                    // touches the column.
                    let padding = tree.records.layout()[node.idx()].padding;
                    let local = text_paint_bbox_local(
                        *local_origin,
                        *align,
                        padding,
                        layout_rect.size,
                        shaped.measured,
                    );
                    let screen = lift_to_screen(local, layout_rect.min, shape_transform, None);
                    inflate_text_damage(screen, shaped.measured, shape_clip)
                }
                ShapeRecord::Polyline {
                    width,
                    cap,
                    join,
                    points,
                    bbox,
                    ..
                } => {
                    // The AA fringe is physical, so inflate only after the
                    // centerline and stroke width reach screen space.
                    let centerline = lift_to_screen(*bbox, layout_rect.min, shape_transform, None);
                    let screen = stroked_bbox(
                        centerline,
                        *width * shape_transform.scale,
                        HALF_FRINGE / display_scale,
                        *cap,
                        (points.len > 2).then_some(*join),
                    );
                    clip_screen(screen, shape_clip)
                }
                ShapeRecord::Curve {
                    width, cap, bbox, ..
                }
                | ShapeRecord::Arc {
                    width, cap, bbox, ..
                } => {
                    let centerline = lift_to_screen(*bbox, layout_rect.min, shape_transform, None);
                    let screen = stroked_bbox(
                        centerline,
                        *width * shape_transform.scale,
                        HALF_FRINGE / display_scale,
                        *cap,
                        None,
                    );
                    clip_screen(screen, shape_clip)
                }
                _ => lift_to_screen(
                    s.bbox_local(layout_rect.size),
                    layout_rect.min,
                    shape_transform,
                    shape_clip,
                ),
            };
            push_paint(arena, &mut union, screen, shape_hashes[idx as usize]);
        }
        debug_assert_eq!(
            text_ord, text_span.len,
            "cascade text count differs from the node's shaped-text span",
        );
    }

    let paints_len = arena.rows.len() as u32 - paints_start;
    arena.node_spans[node.idx()] = Span::new(paints_start, paints_len);
    union.unwrap_or(Rect::ZERO)
}
