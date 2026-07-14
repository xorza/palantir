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

use crate::forest::element::LayoutMode;
use crate::forest::tree::{NodeId, Tree};
use crate::layout::axis::Axis;
use crate::layout::engine::LayoutEngine;
use crate::layout::support::{AxisCtx, TextCtx, leaf_text_shapes, resolve_axis_size};
use crate::layout::types::align::HAlign;
use crate::layout::types::sizing::Sizing;
use crate::layout::{canvas, grid, stack, wrapstack, zstack};
use crate::shape::TextWrap;
use crate::text::ShapeParams;

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
    tc: &TextCtx<'_>,
) -> f32 {
    let style = tree.records.layout()[node.idx()];
    if style.visibility().is_collapsed() {
        return 0.0;
    }
    let bounds = tree.bounds(node);

    let sizing = axis.main_sizing(style.size);
    let margin = axis.spacing(style.margin);
    let min_clamp = axis.main(bounds.min_size);
    let max_clamp = axis.main(bounds.max_size);

    // Hug + Fill both report content-driven intrinsic. Per `intrinsic.md`
    // (next to this file): Fill in intrinsic context returns its content's
    // intrinsic, ignoring weight — `resolve_axis_size` with `available =
    // INFINITY` enforces exactly that (Fill falls back to
    // `content_plus_padding`). Skip the content query and padding read
    // for Fixed: `resolve_axis_size` short-circuits Fixed and never
    // reads `content_plus_padding`.
    let content_plus_padding = match sizing {
        Sizing::Fixed(_) => 0.0,
        Sizing::Hug | Sizing::Fill(_) => {
            let pad = axis.spacing(style.padding);
            content_intrinsic(
                engine,
                tree,
                node,
                axis,
                req,
                tc,
                style.mode,
                style.mode_payload,
            ) + pad
        }
    };

    resolve_axis_size(AxisCtx {
        sizing,
        content_plus_padding,
        available: f32::INFINITY,
        // Intrinsic queries run with `available = INFINITY`; the
        // min-content floor is irrelevant in that branch (no
        // shrinking to apply it to). Pass 0 — the `.max(0)` no-op.
        intrinsic_min: 0.0,
        margin,
        min: min_clamp,
        max: max_clamp,
    })
}

#[allow(clippy::too_many_arguments)]
fn content_intrinsic(
    engine: &mut LayoutEngine,
    tree: &Tree,
    node: NodeId,
    axis: Axis,
    req: LenReq,
    tc: &TextCtx<'_>,
    mode: LayoutMode,
    mode_payload: u16,
) -> f32 {
    match mode {
        LayoutMode::Leaf => leaf(tree, node, axis, req, tc),
        LayoutMode::HStack => stack::intrinsic(engine, tree, node, Axis::X, axis, req, tc),
        LayoutMode::VStack => stack::intrinsic(engine, tree, node, Axis::Y, axis, req, tc),
        LayoutMode::WrapHStack => wrapstack::intrinsic(engine, tree, node, Axis::X, axis, req, tc),
        LayoutMode::WrapVStack => wrapstack::intrinsic(engine, tree, node, Axis::Y, axis, req, tc),
        LayoutMode::ZStack => zstack::intrinsic(engine, tree, node, axis, req, tc),
        LayoutMode::Canvas => canvas::intrinsic(engine, tree, node, axis, req, tc),
        LayoutMode::Grid => grid::intrinsic(engine, tree, node, mode_payload, axis, req, tc),
        // Scroll viewports "want" zero on every panned axis — sizing
        // comes from the viewport's own `Sizing`, never from content.
        // The non-panned axis falls back to a stack intrinsic on the
        // panned axis (pan-Y → stack on Y, pan-X → stack on X). If
        // both axes pan, the answer is unconditionally zero.
        LayoutMode::Scroll => {
            let pan = LayoutMode::pan_mask_from_payload(mode_payload);
            let pan_axis = match axis {
                Axis::X => pan.x,
                Axis::Y => pan.y,
            };
            if pan_axis {
                0.0
            } else {
                let main = if pan.y { Axis::Y } else { Axis::X };
                stack::intrinsic(engine, tree, node, main, axis, req, tc)
            }
        }
    }
}

/// Leaf: walk shapes and aggregate. Only `ShapeRecord::Text` contributes
/// non-zero intrinsics today; other shapes are owner-relative paint and
/// don't drive size. Lives here rather than in a `leaf` module because
/// there isn't one — leaves have no driver, the leaf path is just "ask
/// the recorded shapes."
fn leaf(tree: &Tree, node: NodeId, axis: Axis, req: LenReq, tc: &TextCtx<'_>) -> f32 {
    let wid = tree.records.widget_id()[node.idx()];
    let curr_hash = tree.rollups.node[node.idx()];
    let mut acc = 0.0_f32;
    // Same within-node `ordinal` keying + overflow contract as
    // `LayoutEngine::leaf_content_size` — both walk `leaf_text_shapes`
    // in record order and key the text cache on `(wid, ordinal, hash)`,
    // so the counter must derive identically on both sides.
    let mut ordinal: u16 = 0;
    for ts in leaf_text_shapes(tree, tc, node) {
        let m = tc.shaper.shape_unbounded(
            wid,
            ordinal,
            curr_hash,
            ts.text,
            ts.text_hash,
            ShapeParams {
                font_size_px: ts.font_size_px,
                line_height_px: ts.line_height_px,
                max_width_px: None,
                family: ts.family,
                weight: ts.weight,
                halign: HAlign::Auto,
            },
        );
        let v = match (axis, req) {
            // Non-wrapping text can't break, so its min-content equals its
            // unbroken width. Returning the longest-word width here would
            // let a Hug-track solver shrink the column below the actual
            // floor, and the text would overflow its slot at arrange.
            (Axis::X, LenReq::MinContent) => match ts.wrap {
                TextWrap::WrapWithOverflow => m.intrinsic_min,
                TextWrap::SingleLine => m.size.w,
                // `Wrap` can break inside a word at the glyph level, so its
                // min-content is effectively zero; a truncating run likewise
                // shrinks to nothing. `Scroll` never reshapes, but its owner
                // clips + scrolls the overflow, so the box is free to shrink
                // below the text. In every case the box width wins and the run
                // reflows / cuts / scrolls to it.
                TextWrap::Wrap | TextWrap::Truncate | TextWrap::Ellipsis | TextWrap::Scroll => 0.0,
            },
            // `Scroll` text is scroll content, not layout content: it drives no
            // box width on either axis (matching the zero-width `leaf_content_size`
            // report), so a size-to-content parent doesn't reserve the buffer's
            // natural width for it. The field's width comes from its own sizing /
            // `min_size`. Every other mode wants its full unbroken line.
            (Axis::X, LenReq::MaxContent) => match ts.wrap {
                TextWrap::Scroll => 0.0,
                _ => m.size.w,
            },
            (Axis::Y, _) => m.size.h,
        };
        acc = acc.max(v);
        ordinal = ordinal.checked_add(1).expect(
            "more than 65535 ShapeRecord::Text per leaf — well past anything sane; \
             widen the within-node ordinal width if this trips",
        );
    }
    acc
}

#[cfg(test)]
mod tests {
    use crate::forest::tree::NodeId;
    use crate::layout::intrinsic::*;

    use crate::Ui;
    use crate::forest::Layer;
    use crate::forest::element::Configure;
    use crate::layout::support::TextCtx;
    use crate::layout::types::sizing::Sizing;
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
        let mut ui = Ui::for_test();
        let mut root = NodeId(0);
        ui.run_at(UVec2::new(400, 300), |ui| {
            root = Panel::hstack()
                .auto_id()
                .size((Sizing::FILL, Sizing::Hug))
                .show(ui, |ui| {
                    Text::new("lorem ipsum dolor sit amet")
                        .id_salt("msg")
                        .text_wrap(TextWrap::WrapWithOverflow)
                        .size((Sizing::FILL, Sizing::Hug))
                        .show(ui);
                })
                .node();
        });

        let child = ui.forest.trees[Layer::Main]
            .children(root)
            .map(|c| c.id)
            .next()
            .expect("hstack has child");
        let slot = LenReq::MinContent.slot(Axis::X);
        let cached = ui.layout_engine.scratch.intrinsics[child.idx()][slot];
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
        let mut ui = Ui::for_test();
        let mut root = NodeId(0);
        ui.run_at(UVec2::new(400, 300), |ui| {
            root = Panel::hstack()
                .auto_id()
                .size((Sizing::FILL, Sizing::Hug))
                .show(ui, |ui| {
                    Text::new("hello world")
                        .id_salt("msg")
                        .text_wrap(TextWrap::WrapWithOverflow)
                        .size((Sizing::FILL, Sizing::Hug))
                        .show(ui);
                })
                .node();
        });

        let child = ui.forest.trees[Layer::Main]
            .children(root)
            .map(|c| c.id)
            .next()
            .unwrap();
        let slot = LenReq::MinContent.slot(Axis::X);

        const SENTINEL: f32 = 1234.5;
        ui.layout_engine.scratch.intrinsics[child.idx()][slot] = SENTINEL;

        let arena = ui.ctx.frame_arena.inner();
        let v = ui.layout_engine.intrinsic(
            &ui.forest.trees[Layer::Main],
            child,
            Axis::X,
            LenReq::MinContent,
            &TextCtx {
                bytes: &arena.fmt_scratch,
                shaper: &ui.ctx.shaper,
            },
        );
        drop(arena);
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
        let mut ui = Ui::for_test();
        let mut root = NodeId(0);
        // `run_at` populates `tree.rollups` (leaf intrinsic reads it).
        // Then clear *just the queried slot* on every node so we can
        // observe which nodes the parent query repopulates.
        ui.run_at(UVec2::new(400, 300), |ui| {
            root = Panel::hstack()
                .auto_id()
                .size((Sizing::Hug, Sizing::Hug))
                .show(ui, |ui| {
                    Text::new("abc").id_salt("a").show(ui);
                    Text::new("defgh").id_salt("b").show(ui);
                })
                .node();
        });
        // Drop the measure-cache snapshots so `engine.intrinsic` can't
        // answer the root query from last frame's cached intrinsic — this
        // test pins the *recursive compute* path that populates descendant
        // scratch slots, which the cross-frame lookup would otherwise skip.
        ui.clear_measure_cache();
        let slot = LenReq::MaxContent.slot(Axis::X);
        for entry in ui.layout_engine.scratch.intrinsics.iter_mut() {
            entry[slot] = f32::NAN;
        }

        let arena = ui.ctx.frame_arena.inner();
        let _ = ui.layout_engine.intrinsic(
            &ui.forest.trees[Layer::Main],
            root,
            Axis::X,
            LenReq::MaxContent,
            &TextCtx {
                bytes: &arena.fmt_scratch,
                shaper: &ui.ctx.shaper,
            },
        );
        drop(arena);

        assert!(
            !ui.layout_engine.scratch.intrinsics[root.idx()][slot].is_nan(),
            "root slot must be cached"
        );
        for c in ui.forest.trees[Layer::Main].children(root).map(|c| c.id) {
            assert!(
                !ui.layout_engine.scratch.intrinsics[c.idx()][slot].is_nan(),
                "child {} slot must be cached after parent query",
                c.idx()
            );
        }
    }
}
