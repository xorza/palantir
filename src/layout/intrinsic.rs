//! Intrinsic-dimensions queries — the on-demand `LenReq` API spec'd in
//! `intrinsic.md` (next to this file).
//!
//! This module owns:
//! - The query type `LenReq`.
//! - The central `compute()` dispatch that handles `Sizing` overrides,
//!   padding/margin, and `min_size`/`max_size` clamps before delegating to
//!   each driver's `intrinsic()` for content-driven sizes.
//! - Leaf intrinsics (no driver module owns leaves).
//!
//! Per-driver intrinsic logic (`stack`, `zstack`, `canvas`, `grid`) lives
//! alongside that driver's `measure`/`arrange` in its own module — same
//! per-driver-file convention as the rest of layout.

use super::support::leaf_text_shapes;
use super::{
    Axis, LayoutEngine, LayoutMode, canvas, grid, resolve_axis_size, stack, wrapstack, zstack,
};
use crate::layout::types::sizing::Sizing;
use crate::text::TextMeasurer;
use crate::tree::{NodeId, Tree};

/// Intrinsic content-size kind, per CSS Grid spec terminology.
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub(crate) enum LenReq {
    /// Smallest size the node can occupy without breaking. Text: longest
    /// unbreakable run.
    MinContent,
    /// Size the node "wants" with unlimited room. Text: natural unbroken
    /// width.
    MaxContent,
}

/// Width of the `[f32; SLOT_COUNT]` array on `LayoutScratch.intrinsics`.
/// Equals `LenReq` variants × `Axis` variants. Adding a third variant
/// to either enum must update this constant and `LenReq::slot`; the
/// `const _:` below catches the array overflow at compile time.
pub(crate) const SLOT_COUNT: usize = 4;

impl LenReq {
    /// Index into `LayoutScratch.intrinsics[node]` for `(axis, self)`.
    /// Encoding lives next to the variant set so adding a `LenReq`
    /// surfaces here, not in `mod.rs`.
    #[inline]
    pub(crate) const fn slot(self, axis: Axis) -> usize {
        let a = match axis {
            Axis::X => 0,
            Axis::Y => 1,
        };
        let r = match self {
            LenReq::MinContent => 0,
            LenReq::MaxContent => 1,
        };
        a * 2 + r
    }
}

const _: () = {
    assert!(LenReq::MinContent.slot(Axis::X) < SLOT_COUNT);
    assert!(LenReq::MinContent.slot(Axis::Y) < SLOT_COUNT);
    assert!(LenReq::MaxContent.slot(Axis::X) < SLOT_COUNT);
    assert!(LenReq::MaxContent.slot(Axis::Y) < SLOT_COUNT);
};

/// `(MinContent, MaxContent)` pair for one axis. Returned by
/// [`crate::layout::LayoutEngine::intrinsic_pair`] and the internal
/// `compute_pair` / `leaf_pair` helpers. Named so callers destructure
/// by field rather than positional tuple — `(min, max)` ordering would
/// silently break if anyone ever rearranges it.
#[derive(Copy, Clone, Debug, PartialEq)]
pub(crate) struct IntrinsicBounds {
    pub min: f32,
    pub max: f32,
}

impl IntrinsicBounds {
    pub(crate) const ZERO: Self = Self { min: 0.0, max: 0.0 };
}

/// Outer intrinsic on `axis`: content + padding + margin, respecting the
/// node's `Sizing` override and `min_size` / `max_size` clamps.
///
/// Pure function of the subtree at `node`. Engine caches the result; this
/// function is the cache miss path.
pub(crate) fn compute(
    engine: &mut LayoutEngine,
    tree: &Tree,
    node: NodeId,
    axis: Axis,
    req: LenReq,
    text: &mut TextMeasurer,
) -> f32 {
    if tree.is_collapsed(node) {
        return 0.0;
    }

    let style = *tree.layout(node);
    let extras = tree.read_extras(node);

    let sizing = axis.main_sizing(style.size);
    let pad = axis.spacing(style.padding);
    let margin = axis.spacing(style.margin);
    let min_clamp = axis.main(extras.min_size);
    let max_clamp = axis.main(extras.max_size);

    // Hug + Fill both report content-driven intrinsic. Per `intrinsic.md`
    // (next to this file): Fill in intrinsic context returns its content's
    // intrinsic, ignoring weight — `resolve_axis_size` with `available =
    // INFINITY` enforces exactly that (Fill falls back to `hug_with_margin`).
    // Skip the content query for Fixed: `resolve_axis_size` short-circuits
    // Fixed and never reads `hug_with_margin`. The `+ margin` term gets
    // subtracted back inside `resolve_axis_size` (Hug branch:
    // `hug_with_margin - margin`) and re-added at the end — the round-trip
    // exists so this function and the in-pass measure path can share the
    // same Sizing/clamp logic.
    let hug_with_margin = match sizing {
        Sizing::Fixed(_) => 0.0,
        Sizing::Hug | Sizing::Fill(_) => {
            content_intrinsic(engine, tree, node, axis, req, text, style.mode) + pad + margin
        }
    };

    resolve_axis_size(
        sizing,
        hug_with_margin,
        f32::INFINITY,
        margin,
        min_clamp,
        max_clamp,
    )
}

fn content_intrinsic(
    engine: &mut LayoutEngine,
    tree: &Tree,
    node: NodeId,
    axis: Axis,
    req: LenReq,
    text: &mut TextMeasurer,
    mode: LayoutMode,
) -> f32 {
    match mode {
        LayoutMode::Leaf => leaf(tree, node, axis, req, text),
        LayoutMode::HStack => stack::intrinsic(engine, tree, node, Axis::X, axis, req, text),
        LayoutMode::VStack => stack::intrinsic(engine, tree, node, Axis::Y, axis, req, text),
        LayoutMode::WrapHStack => {
            wrapstack::intrinsic(engine, tree, node, Axis::X, axis, req, text)
        }
        LayoutMode::WrapVStack => {
            wrapstack::intrinsic(engine, tree, node, Axis::Y, axis, req, text)
        }
        LayoutMode::ZStack => zstack::intrinsic(engine, tree, node, axis, req, text),
        LayoutMode::Canvas => canvas::intrinsic(engine, tree, node, axis, req, text),
        LayoutMode::Grid(idx) => grid::intrinsic(engine, tree, node, idx, axis, req, text),
    }
}

/// Leaf: walk shapes and aggregate. Only `Shape::Text` contributes
/// non-zero intrinsics today; other shapes are owner-relative paint and
/// don't drive size. Lives here rather than in a `leaf` module because
/// there isn't one — leaves have no driver, the leaf path is just "ask
/// the recorded shapes."
fn leaf(tree: &Tree, node: NodeId, axis: Axis, req: LenReq, text: &mut TextMeasurer) -> f32 {
    let wid = tree.widget_ids[node.index()];
    let curr_hash = tree.hashes[node.index()];
    let mut acc = 0.0_f32;
    for (src, font_size_px, _wrap) in leaf_text_shapes(tree, node) {
        let m = text.shape_unbounded(wid, curr_hash, src, font_size_px);
        let v = match (axis, req) {
            (Axis::X, LenReq::MinContent) => m.intrinsic_min,
            (Axis::X, LenReq::MaxContent) => m.size.w,
            (Axis::Y, _) => m.size.h,
        };
        acc = acc.max(v);
    }
    acc
}

/// Leaf intrinsic for both bounds at once. Single `shape_unbounded`
/// call extracts `intrinsic_min` and `size.w` from the same shaped
/// result; today's two-call form goes through the TextMeasurer cache
/// twice for the second hit.
fn leaf_pair(tree: &Tree, node: NodeId, axis: Axis, text: &mut TextMeasurer) -> IntrinsicBounds {
    let wid = tree.widget_ids[node.index()];
    let curr_hash = tree.hashes[node.index()];
    let mut acc = IntrinsicBounds::ZERO;
    for (src, font_size_px, _wrap) in leaf_text_shapes(tree, node) {
        let m = text.shape_unbounded(wid, curr_hash, src, font_size_px);
        let bounds = match axis {
            Axis::X => IntrinsicBounds {
                min: m.intrinsic_min,
                max: m.size.w,
            },
            Axis::Y => IntrinsicBounds {
                min: m.size.h,
                max: m.size.h,
            },
        };
        acc.min = acc.min.max(bounds.min);
        acc.max = acc.max.max(bounds.max);
    }
    acc
}

/// Both bounds at once. Caches both slots after the walk. Today's
/// implementation only batches the *leaf* shaper call; non-leaf
/// dispatchers fall back to two single-bound walks. That still saves
/// the leaf-side TextMeasurer cache probe (the bigger cost on
/// text-heavy trees) and centralizes the cache write so future driver-
/// level batching is a single point of change.
pub(crate) fn compute_pair(
    engine: &mut LayoutEngine,
    tree: &Tree,
    node: NodeId,
    axis: Axis,
    text: &mut TextMeasurer,
) -> IntrinsicBounds {
    if tree.is_collapsed(node) {
        return IntrinsicBounds::ZERO;
    }

    let style = *tree.layout(node);
    let extras = tree.read_extras(node);

    let sizing = axis.main_sizing(style.size);
    let pad = axis.spacing(style.padding);
    let margin = axis.spacing(style.margin);
    let min_clamp = axis.main(extras.min_size);
    let max_clamp = axis.main(extras.max_size);

    let content = match sizing {
        Sizing::Fixed(_) => IntrinsicBounds::ZERO,
        Sizing::Hug | Sizing::Fill(_) => {
            if matches!(style.mode, LayoutMode::Leaf) {
                leaf_pair(tree, node, axis, text)
            } else {
                // Driver dispatch isn't batched yet — fall through to
                // two single-bound walks. Still cheaper than the user
                // calling `intrinsic` twice because the cache writes
                // happen here, and the leaf-level shaper call uses
                // `leaf_pair` when it bottoms out via `engine.intrinsic`.
                IntrinsicBounds {
                    min: content_intrinsic(
                        engine,
                        tree,
                        node,
                        axis,
                        LenReq::MinContent,
                        text,
                        style.mode,
                    ),
                    max: content_intrinsic(
                        engine,
                        tree,
                        node,
                        axis,
                        LenReq::MaxContent,
                        text,
                        style.mode,
                    ),
                }
            }
        }
    };

    let resolve = |content: f32| {
        let hug_with_margin = content + pad + margin;
        resolve_axis_size(
            sizing,
            hug_with_margin,
            f32::INFINITY,
            margin,
            min_clamp,
            max_clamp,
        )
    };
    IntrinsicBounds {
        min: resolve(content.min),
        max: resolve(content.max),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Ui;
    use crate::layout::types::{display::Display, sizing::Sizing};
    use crate::tree::element::Configure;
    use crate::widgets::{panel::Panel, text::Text};
    use glam::UVec2;

    /// Driver-triggered intrinsic queries during `run` must populate
    /// the per-node cache. Without this, every `engine.intrinsic` call
    /// would recompute from scratch — the 9% intrinsic cost in the
    /// layout bench would balloon.
    ///
    /// Uses the HStack-with-Fill-wrap pattern: pass-2 of
    /// `stack::measure` queries `MinContent` on each Fill child.
    #[test]
    fn intrinsic_cache_populated_after_run() {
        let mut ui = Ui::new();
        ui.begin_frame(Display::from_physical(UVec2::new(400, 300), 1.0));
        let root = Panel::hstack()
            .size((Sizing::FILL, Sizing::Hug))
            .show(&mut ui, |ui| {
                Text::new("lorem ipsum dolor sit amet")
                    .with_id("msg")
                    .wrapping()
                    .size((Sizing::FILL, Sizing::Hug))
                    .show(ui);
            })
            .node;
        ui.end_frame();

        let child = ui.tree.children(root).next().expect("hstack has child");
        let slot = LenReq::MinContent.slot(Axis::X);
        let cached = ui.layout_engine.scratch.intrinsics[child.index()][slot];
        assert!(
            !cached.is_nan(),
            "MinContent X for the Fill+wrap child must be cached after run"
        );
    }

    /// `engine.intrinsic` must short-circuit on cache hit. We poison
    /// the slot with a sentinel and verify the next query returns it
    /// — a recompute would overwrite the sentinel with the real value.
    #[test]
    fn intrinsic_query_short_circuits_on_cache_hit() {
        let mut ui = Ui::new();
        ui.begin_frame(Display::from_physical(UVec2::new(400, 300), 1.0));
        let root = Panel::hstack()
            .size((Sizing::FILL, Sizing::Hug))
            .show(&mut ui, |ui| {
                Text::new("hello world")
                    .with_id("msg")
                    .wrapping()
                    .size((Sizing::FILL, Sizing::Hug))
                    .show(ui);
            })
            .node;
        ui.end_frame();

        let child = ui.tree.children(root).next().unwrap();
        let slot = LenReq::MinContent.slot(Axis::X);

        const SENTINEL: f32 = 1234.5;
        ui.layout_engine.scratch.intrinsics[child.index()][slot] = SENTINEL;

        let v =
            ui.layout_engine
                .intrinsic(&ui.tree, child, Axis::X, LenReq::MinContent, &mut ui.text);
        assert_eq!(
            v, SENTINEL,
            "cache hit must return the stored value verbatim, not recompute"
        );
    }

    /// Recursive intrinsic queries must populate descendant slots too,
    /// not just the queried node — `stack::intrinsic` etc. recurse
    /// through `engine.intrinsic`, which writes the cache at every
    /// level. Without this, deep trees would re-walk on every parent
    /// query.
    #[test]
    fn parent_intrinsic_query_populates_descendant_cache() {
        let mut ui = Ui::new();
        ui.begin_frame(Display::from_physical(UVec2::new(400, 300), 1.0));
        let root = Panel::hstack()
            .size((Sizing::Hug, Sizing::Hug))
            .show(&mut ui, |ui| {
                Text::new("abc").with_id("a").show(ui);
                Text::new("defgh").with_id("b").show(ui);
            })
            .node;
        // `end_frame` populates `tree.hashes` (leaf intrinsic reads it).
        // Then clear *just the queried slot* on every node so we can
        // observe which nodes the parent query repopulates.
        ui.end_frame();
        let slot = LenReq::MaxContent.slot(Axis::X);
        for entry in ui.layout_engine.scratch.intrinsics.iter_mut() {
            entry[slot] = f32::NAN;
        }

        let _ =
            ui.layout_engine
                .intrinsic(&ui.tree, root, Axis::X, LenReq::MaxContent, &mut ui.text);

        assert!(
            !ui.layout_engine.scratch.intrinsics[root.index()][slot].is_nan(),
            "root slot must be cached"
        );
        for c in ui.tree.children(root) {
            assert!(
                !ui.layout_engine.scratch.intrinsics[c.index()][slot].is_nan(),
                "child {} slot must be cached after parent query",
                c.index()
            );
        }
    }
}
