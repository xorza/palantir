//! The cascade walk: a per-tree pre-order traversal that recomputes each
//! node's cascade output, consulting the [`reuse`](super::reuse) gate to
//! bulk-copy unchanged subtrees instead. `run_tree` is the skeleton (pop
//! → skip-gate → recompute); `recascade_node` is the per-node compute.
//! A `#[cfg(test)]` oracle (`cross_check`) recomputes from scratch and
//! asserts the incremental walk is byte-identical.

use super::cascade_input::{build_cascade_prefix, finish_cascade_input};
use super::paint_rect::{PaintRectCtx, compute_paint_rect};
use super::reuse::{CascadeSnapshot, PrevTree, copy_subtree};
use super::{Cascades, EntryRow};
use crate::common::hash::Hasher;
use crate::forest::Forest;
use crate::forest::Layer;
use crate::forest::per_layer::PerLayer;
use crate::forest::tree::{NodeId, Tree};
use crate::input::sense::Sense;
use crate::layout::LayerLayout;
use crate::primitives::rect::Rect;
use crate::primitives::transform::TranslateScale;

pub(crate) struct Frame {
    transform: TranslateScale,
    clip: Option<Rect>,
    disabled: bool,
    invisible: bool,
    /// True when this node's inherited context (parent transform / clip
    /// / disabled / invisible) and its own arranged origin + authoring
    /// are unchanged from last frame — so a descendant with an unchanged
    /// `subtree_hash` and origin can be skipped (its cascade output is
    /// provably identical). Always false on the full-recompute path.
    ctx_unchanged: bool,
    subtree_end: u32,
    /// Node index this frame represents — used to write back
    /// `subtree_paint_rect` into `Cascades::subtree_paint_rects` when
    /// this frame is popped (its subtree has been fully visited).
    node_idx: usize,
    /// Running union of this node's own `paint_rect` and the
    /// `subtree_paint_rect` of every descendant whose subtree has
    /// already been folded in. Each pop unions this into the new
    /// top frame so the rollup ripples upward to the root.
    subtree_paint_rect: Rect,
    /// FxHasher state pre-populated with this frame's ancestor-derived
    /// hash inputs (transform / clip / disabled / invisible). Cloned
    /// once per descendant to seed `cascade_input` — descendants only
    /// fold in their own `layout_rect`, avoiding a re-hash of the 32 B
    /// ancestor prefix per node. See `finish_cascade_input`.
    cascade_prefix: Hasher,
}

/// Walk telemetry: how many subtrees were skipped (bulk-copied) vs nodes
/// recomputed. Read by tests to assert the skip gate actually fires;
/// the gated `CascadesEngine::dbg` field is the only reader.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct WalkStats {
    /// Whether `run` took the incremental path (vs full recompute).
    /// Test-only: set in `run`'s `cfg(test)` block and read by tests via
    /// `CascadesEngine::dbg`; the walk itself never touches it.
    #[cfg(test)]
    pub(crate) incremental: bool,
    pub(crate) skipped: u32,
    pub(crate) recascaded: u32,
}

/// The per-layer handles the walk threads through every node: the source
/// `tree` + arranged `layout`, and which `layer`'s `Cascades` columns to
/// write. `Copy` (all refs + a `Layer`), so `recascade_node` takes it by
/// value.
#[derive(Clone, Copy)]
struct LayerView<'a> {
    tree: &'a Tree,
    layout: &'a LayerLayout,
    layer: Layer,
}

/// What a node inherits from its parent frame (or the root defaults):
/// the composed `transform` / `clip` it extends, the `disabled` /
/// `invisible` flags it ORs into, the hash `prefix` it seeds its
/// `cascade_input` from, and whether all of that is unchanged since last
/// frame (`ctx_ok`) — which, paired with an unchanged subtree, is what
/// lets a child be skipped.
#[derive(Clone, Copy)]
struct Inherited<'a> {
    transform: TranslateScale,
    clip: Option<Rect>,
    disabled: bool,
    invisible: bool,
    prefix: &'a Hasher,
    ctx_ok: bool,
}

/// Per-layer walk driver shared by the live run and the test
/// cross-check. `prev: Some` enables the incremental skip; `None`
/// recomputes every node. Returns aggregate [`WalkStats`].
pub(crate) fn run_pass(
    forest: &Forest,
    layers: &PerLayer<LayerLayout>,
    out: &mut Cascades,
    prev: Option<(&Cascades, &PerLayer<Vec<CascadeSnapshot>>)>,
    stack: &mut Vec<Frame>,
    snap_out: &mut PerLayer<Vec<CascadeSnapshot>>,
) -> WalkStats {
    let total: usize = forest.trees.iter().map(|t| t.records.len()).sum();
    out.entries.clear();
    out.entries.reserve(total);

    let mut stats = WalkStats::default();
    for (layer, tree) in forest.iter_paint_order() {
        let layer_layout = &layers[layer];
        let n = tree.records.len();
        let entries_base = out.entries.len() as u32;
        out.layers[layer].reset_for(n, entries_base);
        let snap_layer = &mut snap_out[layer];
        snap_layer.clear();
        snap_layer.reserve(n);
        stack.clear();
        let prev_tree = prev.map(|(pc, ps)| PrevTree {
            cascades: pc,
            snap: ps[layer].as_slice(),
        });
        run_tree(
            tree,
            layer_layout,
            out,
            layer,
            stack,
            prev_tree,
            snap_layer,
            &mut stats,
        );
        // Invariant guarding `Cascades::entry_idx_of`'s
        // `entries_base + node.0` arithmetic: every node in
        // `tree.records` must push exactly one `EntryRow` (whether
        // recomputed or copied). A skip that miscounts, or an
        // early-continue that doesn't push, would silently shift every
        // later widget's entry by one. Release `assert!` — the operands
        // are already loaded, the equality is a single compare.
        assert_eq!(
            out.entries.len() as u32 - entries_base,
            n as u32,
            "run_tree produced {} entries for layer with {n} nodes — every record must yield exactly one row to keep entries_base + node.0 valid",
            out.entries.len() as u32 - entries_base,
        );
        // The folded snapshot must likewise cover every node exactly
        // once, so next frame's NodeId-indexed gate lines up.
        debug_assert_eq!(
            snap_layer.len(),
            n,
            "run_tree wrote {} snapshots for {n} nodes",
            snap_layer.len(),
        );
    }
    stats
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
        parent.subtree_paint_rect = parent.subtree_paint_rect.union(popped.subtree_paint_rect);
    }
}

// `run_tree` drives the walk skeleton (pop → skip-gate → recompute);
// `recascade_node` does the per-node computation. The skeleton still
// takes 8 args — read inputs, the `stack` scratch, the `prev` reuse
// source, and three output sinks (cascades, folded snapshot, stats),
// which alias disjoint storage and would only gain reborrow ceremony if
// bundled.
#[allow(clippy::too_many_arguments)]
fn run_tree(
    tree: &Tree,
    layout: &LayerLayout,
    cascades: &mut Cascades,
    layer: Layer,
    stack: &mut Vec<Frame>,
    prev: Option<PrevTree<'_>>,
    snap_out: &mut Vec<CascadeSnapshot>,
    stats: &mut WalkStats,
) {
    let n = tree.records.len() as u32;
    let ends = tree.records.subtree_end();
    let root_prefix = build_cascade_prefix(TranslateScale::IDENTITY, None, false, false);
    let view = LayerView {
        tree,
        layout,
        layer,
    };

    let mut i: u32 = 0;
    while i < n {
        // Pop completed frames, rolling each up into its parent.
        while let Some(top) = stack.last() {
            if i < top.subtree_end {
                break;
            }
            let popped = stack.pop().unwrap();
            finalize_frame(
                stack,
                &mut cascades.layers[layer].subtree_paint_rects,
                popped,
            );
        }
        let iu = i as usize;
        let layout_rect = layout.rect[iu];
        // `.end()` strips the packed grid flag — downstream uses (walk
        // cursor, leaf compare) need the clean pre-order end.
        let subtree_end = ends[iu].end();
        // Root inherits a constant identity context, so it's always
        // "unchanged" — gated on `prev` so the very first frame (no prev
        // to copy) still recomputes.
        let parent_ctx_ok = stack.last().map_or(prev.is_some(), |p| p.ctx_unchanged);

        // Incremental skip: when the inherited context, this subtree's
        // authoring (`subtree_hash`), and its arranged rect all match
        // last frame, the whole subtree's cascade output is identical —
        // bulk-copy it and jump past it. `subtree_hash` folds every
        // descendant's authoring (incl. transforms, so a scroll shift
        // dirties it); the rect compare catches a Fill-sibling reflow it
        // can't see. Gated on `parent_ctx_ok` first so a node under a
        // changed ancestor never even loads its snapshot.
        if let Some(prev) = prev
            && parent_ctx_ok
        {
            let snap = prev.snap[iu];
            if tree.rollups.subtree[iu] == snap.subtree_hash && layout_rect == snap.rect {
                copy_subtree(prev, cascades, snap_out, layer, iu, subtree_end as usize);
                if let Some(top) = stack.last_mut() {
                    top.subtree_paint_rect = top
                        .subtree_paint_rect
                        .union(prev.cascades.layers[layer].subtree_paint_rects[iu]);
                }
                stats.skipped += 1;
                i = subtree_end;
                continue;
            }
        }
        stats.recascaded += 1;
        // Built here (after the gate) rather than up top so the skip
        // path's `stack.last_mut()` doesn't collide with the
        // `&p.cascade_prefix` borrow this holds.
        let inherited = match stack.last() {
            Some(p) => Inherited {
                transform: p.transform,
                clip: p.clip,
                disabled: p.disabled,
                invisible: p.invisible,
                prefix: &p.cascade_prefix,
                ctx_ok: parent_ctx_ok,
            },
            None => Inherited {
                transform: TranslateScale::IDENTITY,
                clip: None,
                disabled: false,
                invisible: false,
                prefix: &root_prefix,
                ctx_ok: parent_ctx_ok,
            },
        };
        let frame = recascade_node(view, NodeId(i), inherited, prev, cascades, snap_out);
        stack.push(frame);
        i += 1;
    }
    // Drain frames whose subtree extends to the end of the tree —
    // they never hit the `< top.subtree_end` exit at the loop head.
    while let Some(popped) = stack.pop() {
        finalize_frame(
            stack,
            &mut cascades.layers[layer].subtree_paint_rects,
            popped,
        );
    }
}

/// Recompute one node's cascade output from scratch — its `cascade_input`
/// hash, hit entry, paint rows + subtree-paint seed, and the gate
/// snapshot for next frame — and return the [`Frame`] the caller pushes
/// (carrying this node's composed transform / clip / flags and its
/// context-unchanged bit down to its children). The recompute arm of the
/// walk; a skipped subtree bypasses all of this via [`copy_subtree`].
fn recascade_node(
    view: LayerView<'_>,
    node: NodeId,
    inherited: Inherited<'_>,
    prev: Option<PrevTree<'_>>,
    cascades: &mut Cascades,
    snap_out: &mut Vec<CascadeSnapshot>,
) -> Frame {
    let LayerView {
        tree,
        layout,
        layer,
    } = view;
    let iu = node.idx();
    let layout_rect = layout.rect[iu];
    let subtree_end = tree.subtree_end_of(iu);
    let subtree_hash = tree.rollups.subtree[iu];
    let node_hash = tree.rollups.node[iu];
    let attrs = tree.records.attrs()[iu];
    let layout_core = &tree.records.layout()[iu];

    let disabled = inherited.disabled || attrs.is_disabled();
    let invisible = inherited.invisible || !layout_core.visibility().is_visible();
    let wid = tree.records.widget_id()[iu];

    let screen_rect = inherited.transform.apply_rect(layout_rect);
    let visible_rect = inherited
        .clip
        .map_or(screen_rect, |c| screen_rect.intersect(c));
    // The transform descendants inherit *and* direct shapes paint under
    // (the `Panel::transform` contract): `parent ∘ self_anchored`.
    // `transform_of` is a sparse-column probe and `compose` is 3×mul+3×add,
    // so the `None` arm (most nodes have no transform) skips the compose
    // entirely — the steady-state path. `compute_paint_rect` reuses this
    // as its `shape_transform` rather than recomposing.
    //
    // Scale pivots about the node's own `layout_rect.min`, not (0, 0);
    // `anchored_at` cancels the `panel.min * (1 - scale)` drift a raw
    // compose against absolute-coord layout rects would introduce
    // (identity-preserving — no-op at `scale == 1`).
    let node_transform = tree.transform_of(node);
    let desc_transform = match node_transform {
        Some(t) => inherited.transform.compose(t.anchored_at(layout_rect.min)),
        None => inherited.transform,
    };
    let clips = attrs.clip_mode().is_clip();
    // Encoder's clip mask is `rect.deflated_by(padding)`, pushed
    // **before** the body; direct shapes and descendants both paint
    // inside it. Mirror that here so per-shape damage rects and inherited
    // child clips reflect what actually paints — otherwise a TextEdit's
    // tall text shape reports damage well past the editor's rect on every
    // scroll tick.
    let shape_clip = if clips {
        let padding = layout_core.padding;
        let mask_local = layout_rect.deflated_by(padding);
        let mask_screen = inherited.transform.apply_rect(mask_local);
        Some(
            inherited
                .clip
                .map_or(mask_screen, |c| mask_screen.intersect(c)),
        )
    } else {
        inherited.clip
    };
    let paint_rect = compute_paint_rect(
        PaintRectCtx {
            tree,
            layout,
            node,
            layout_rect,
            parent_transform: inherited.transform,
            parent_clip: inherited.clip,
            shape_clip,
            shape_transform: desc_transform,
            clips,
        },
        &mut cascades.layers[layer].paint_arena,
    );
    // Invisible nodes never paint, so seeding their subtree rollup with
    // `Rect::ZERO` keeps a long-lived hidden subtree from inflating the
    // ancestor's `subtree_paint_rect` (and killing the encoder's viewport
    // / damage cull there). Visibility is in `cascade_input` regardless.
    let subtree_seed = if invisible { Rect::ZERO } else { paint_rect };
    cascades.layers[layer]
        .cascade_inputs
        .push(finish_cascade_input(
            inherited.prefix,
            layout_rect,
            invisible,
        ));
    cascades.layers[layer]
        .subtree_paint_rects
        .push(subtree_seed);

    // Descendants inherit the deflated-mask clip — the same value the
    // direct shapes were clipped to and the encoder pushes before the
    // body.
    let desc_clip = shape_clip;
    let cascaded_off = disabled || invisible;
    let sense = if cascaded_off {
        Sense::NONE
    } else {
        attrs.sense()
    };
    let focusable = !cascaded_off && attrs.is_focusable();
    cascades.push_entry(EntryRow {
        widget_id: wid,
        rect: visible_rect,
        sense,
        focusable,
        disabled,
        layout_rect,
    });

    // Stamp this node's gate inputs for next frame (rollups + rect are
    // already in cache from the work above). A skip copies the whole
    // subtree's snapshots in `copy_subtree`, so every node is written
    // exactly once, in NodeId order.
    snap_out.push(CascadeSnapshot {
        node_hash,
        subtree_hash,
        rect: layout_rect,
        widget_id: wid,
    });

    // Leaves can't be a parent prefix for anyone — skip the 32 B
    // prefix-hash work, push a fresh-state `Hasher` placeholder
    // (`Hasher::new()` is just `FxHasher { hash: 0 }`, ~free).
    let is_leaf = subtree_end == node.0 + 1;
    let cascade_prefix = if is_leaf {
        Hasher::new()
    } else {
        build_cascade_prefix(desc_transform, desc_clip, disabled, invisible)
    };
    // Children inherit an unchanged context iff the inherited context
    // already was, this node's own authoring (`node_hash` — its transform
    // / clip / disabled / visibility) is unchanged, and — *only* when the
    // node passes a rect-derived value down — its arranged rect is
    // unchanged. A node feeds its rect into its children's context only
    // via a transform (anchored at its origin) or a clip (screen clip =
    // parent·rect); a plain container that merely resized hands children
    // an unchanged transform/clip, so its rect is irrelevant to them. A
    // child that itself *moved* is still caught by the skip gate's own
    // rect compare. A deeper subtree change leaves this true (so the
    // changed node's siblings stay skippable). Short-circuit on
    // `inherited.ctx_ok` so a node under a changed ancestor doesn't load
    // its snapshot just to discard the result.
    let ctx_depends_on_rect = node_transform.is_some() || clips;
    let child_ctx_ok = match prev {
        Some(prev) if inherited.ctx_ok => {
            let snap = prev.snap[iu];
            node_hash == snap.node_hash && (!ctx_depends_on_rect || layout_rect == snap.rect)
        }
        _ => false,
    };
    Frame {
        transform: desc_transform,
        clip: desc_clip,
        disabled,
        invisible,
        ctx_unchanged: child_ctx_ok,
        subtree_end,
        node_idx: iu,
        subtree_paint_rect: subtree_seed,
        cascade_prefix,
    }
}

/// Oracle for the incremental path: recompute the whole cascade from
/// scratch into a throwaway buffer and assert it's byte-identical to
/// what the reuse walk produced. Runs on every incremental frame under
/// test, so the entire frame-driving test suite verifies reuse
/// correctness across whatever topologies it exercises.
#[cfg(test)]
pub(crate) fn cross_check(
    forest: &Forest,
    layers: &PerLayer<LayerLayout>,
    built: &Cascades,
    stack: &mut Vec<Frame>,
) {
    let mut scratch = Cascades::default();
    let mut scratch_snap = PerLayer::<Vec<CascadeSnapshot>>::default();
    run_pass(forest, layers, &mut scratch, None, stack, &mut scratch_snap);
    scratch.by_id.clone_from(&forest.ids.curr);
    assert_cascades_eq(built, &scratch);
}

/// Field-by-field equality of two `Cascades` (neither it nor its
/// columns derive `PartialEq` — `entries` is a `Soa`). Used only by
/// [`cross_check`].
#[cfg(test)]
fn assert_cascades_eq(got: &Cascades, want: &Cascades) {
    for (layer, gl) in got.layers.iter_paint_order() {
        let wl = &want.layers[layer];
        assert_eq!(
            gl.cascade_inputs, wl.cascade_inputs,
            "cascade_inputs mismatch @ {layer:?}"
        );
        assert_eq!(
            gl.subtree_paint_rects, wl.subtree_paint_rects,
            "subtree_paint_rects mismatch @ {layer:?}"
        );
        assert_eq!(
            gl.paint_arena.rows, wl.paint_arena.rows,
            "paint rows mismatch @ {layer:?}"
        );
        assert_eq!(
            gl.paint_arena.node_spans, wl.paint_arena.node_spans,
            "node_spans mismatch @ {layer:?}"
        );
        assert_eq!(
            gl.entries_base, wl.entries_base,
            "entries_base mismatch @ {layer:?}"
        );
    }
    let (ge, we) = (&got.entries, &want.entries);
    assert_eq!(ge.len(), we.len(), "entries len mismatch");
    assert_eq!(ge.widget_id(), we.widget_id(), "entries.widget_id mismatch");
    assert_eq!(ge.rect(), we.rect(), "entries.rect mismatch");
    assert_eq!(ge.sense(), we.sense(), "entries.sense mismatch");
    assert_eq!(ge.focusable(), we.focusable(), "entries.focusable mismatch");
    assert_eq!(ge.disabled(), we.disabled(), "entries.disabled mismatch");
    assert_eq!(
        ge.layout_rect(),
        we.layout_rect(),
        "entries.layout_rect mismatch"
    );
}
