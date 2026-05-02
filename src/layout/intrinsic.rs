//! Intrinsic-dimensions queries — the on-demand `LenReq` API spec'd in
//! `intrinsic.md` (next to this file).
//!
//! This module owns:
//! - The query types (`LenReq`, `IntrinsicQuery`).
//! - The central `compute()` dispatch that handles `Sizing` overrides,
//!   padding/margin, and `min_size`/`max_size` clamps before delegating to
//!   each driver's `intrinsic()` for content-driven sizes.
//! - Leaf intrinsics (no driver module owns leaves).
//!
//! Per-driver intrinsic logic (`stack`, `zstack`, `canvas`, `grid`) lives
//! alongside that driver's `measure`/`arrange` in its own module — same
//! per-driver-file convention as the rest of layout.

use super::{Axis, LayoutEngine, LayoutMode, canvas, grid, resolve_axis_size, stack, zstack};
use crate::primitives::Sizing;
use crate::shape::Shape;
use crate::text::TextMeasurer;
use crate::tree::{NodeId, Tree};

/// Intrinsic content-size kind, per CSS Grid spec terminology.
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub enum LenReq {
    /// Smallest size the node can occupy without breaking. Text: longest
    /// unbreakable run.
    MinContent,
    /// Size the node "wants" with unlimited room. Text: natural unbroken
    /// width.
    MaxContent,
}

/// One intrinsic query — what the cache keys on. `f32` answers are
/// indexed by these.
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub struct IntrinsicQuery {
    pub node: NodeId,
    pub axis: Axis,
    pub req: LenReq,
}

/// Outer intrinsic on `axis`: content + padding + margin, respecting the
/// node's `Sizing` override and `min_size` / `max_size` clamps.
///
/// Pure function of the subtree at `node`. Engine caches the result; this
/// function is the cache miss path.
pub(super) fn compute(
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
    // INFINITY` enforces exactly that (Fill falls back to `hug_outer`).
    // Skip the content query for Fixed: `resolve_axis_size` short-circuits
    // Fixed and never reads `hug_outer`.
    let hug_outer = match sizing {
        Sizing::Fixed(_) => 0.0,
        Sizing::Hug | Sizing::Fill(_) => {
            content_intrinsic(engine, tree, node, axis, req, text, style.mode) + pad + margin
        }
    };

    resolve_axis_size(
        sizing,
        hug_outer,
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
    let mut acc = 0.0_f32;
    for shape in tree.shapes_of(node) {
        if let Shape::Text {
            text: src,
            font_size_px,
            ..
        } = shape
        {
            let m = text.measure(src, *font_size_px, None);
            let v = match (axis, req) {
                (Axis::X, LenReq::MinContent) => m.intrinsic_min,
                (Axis::X, LenReq::MaxContent) => m.size.w,
                (Axis::Y, _) => m.size.h,
            };
            acc = acc.max(v);
        }
    }
    acc
}
