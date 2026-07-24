//! Intrinsic-dimensions queries — the on-demand `LenReq` API spec'd in
//! `../intrinsic.md`.
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

use crate::layout::axis::Axis;
use crate::layout::engine::LayoutEngine;
use crate::layout::support::{AxisCtx, TextShapeInput, leaf_text_shapes, resolve_axis_size};
use crate::layout::types::layout_mode::LayoutMode;
use crate::layout::{canvas, grid, stack, wrapstack, zstack};
use crate::primitives::interned_str::InternedText;
use crate::primitives::widget_id::WidgetId;
use crate::scene::node::columns::LayoutCore;
use crate::scene::tree::Tree;
use crate::scene::tree::node::NodeId;
use crate::text::wrap::TextWrap;
use crate::text::{TextMeasurement, TextRunIdentity};

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

#[derive(Copy, Clone, Debug, PartialEq)]
pub(crate) struct IntrinsicRange {
    pub(crate) min: f32,
    pub(crate) max: f32,
}

impl IntrinsicRange {
    pub(crate) const ZERO: Self = Self { min: 0.0, max: 0.0 };
}

/// `RANGE` keeps the recursive hot path free of per-node mode branches.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) struct IntrinsicQuery<const RANGE: bool> {
    single_req: LenReq,
}

impl<const RANGE: bool> IntrinsicQuery<RANGE> {
    #[inline]
    pub(crate) fn includes(self, req: LenReq) -> bool {
        RANGE || self.single_req == req
    }

    #[inline]
    pub(crate) fn child(
        self,
        engine: &mut LayoutEngine,
        tree: &Tree,
        node: NodeId,
        axis: Axis,
        interned_text: &InternedText<'_>,
    ) -> IntrinsicRange {
        if RANGE {
            engine.intrinsic_range(tree, node, axis, interned_text)
        } else {
            let value = engine.intrinsic(tree, node, axis, self.single_req, interned_text);
            match self.single_req {
                LenReq::MinContent => IntrinsicRange {
                    min: value,
                    max: 0.0,
                },
                LenReq::MaxContent => IntrinsicRange {
                    min: 0.0,
                    max: value,
                },
            }
        }
    }
}

impl IntrinsicQuery<false> {
    pub(crate) const fn single(req: LenReq) -> Self {
        Self { single_req: req }
    }
}

impl IntrinsicQuery<true> {
    pub(crate) const fn range() -> Self {
        Self {
            single_req: LenReq::MinContent,
        }
    }
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
pub(crate) fn compute<const RANGE: bool>(
    engine: &mut LayoutEngine,
    tree: &Tree,
    node: NodeId,
    axis: Axis,
    query: IntrinsicQuery<RANGE>,
    interned_text: &InternedText<'_>,
) -> IntrinsicRange {
    let layout = tree.records.layout()[node.idx()];
    if layout.meta.visibility().is_collapsed() {
        return IntrinsicRange::ZERO;
    }
    let bounds = tree.bounds(node);

    let sizing = axis.main_sizing(layout.size);
    let margin = axis.spacing(layout.margin);
    let min_clamp = axis.main(bounds.min_size);
    let max_clamp = axis.main(bounds.max_size);

    // Hug + Fill both report content-driven intrinsic. Per
    // `../intrinsic.md`: Fill in intrinsic context returns its content's
    // intrinsic, ignoring weight — `resolve_axis_size` with `available =
    // INFINITY` enforces exactly that (Fill falls back to
    // `content_plus_padding`). Skip the content query and padding read
    // for Fixed: `resolve_axis_size` short-circuits Fixed and never
    // reads `content_plus_padding`.
    let mut content = if sizing.fixed_value().is_some() {
        IntrinsicRange::ZERO
    } else {
        let mut content = content_intrinsic(engine, tree, node, axis, query, interned_text, layout);
        let pad = axis.spacing(layout.padding);
        if query.includes(LenReq::MinContent) {
            content.min += pad;
        }
        if query.includes(LenReq::MaxContent) {
            content.max += pad;
        }
        content
    };

    for (req, value) in [
        (LenReq::MinContent, &mut content.min),
        (LenReq::MaxContent, &mut content.max),
    ] {
        if !query.includes(req) {
            continue;
        }
        *value = resolve_axis_size(AxisCtx {
            sizing,
            content_plus_padding: *value,
            available: f32::INFINITY,
            intrinsic_min: 0.0,
            margin,
            min: min_clamp,
            max: max_clamp,
        });
    }
    content
}

#[allow(clippy::too_many_arguments)]
fn content_intrinsic<const RANGE: bool>(
    engine: &mut LayoutEngine,
    tree: &Tree,
    node: NodeId,
    axis: Axis,
    query: IntrinsicQuery<RANGE>,
    interned_text: &InternedText<'_>,
    layout: LayoutCore,
) -> IntrinsicRange {
    match LayoutMode::from(layout.meta) {
        LayoutMode::Leaf => leaf(engine, tree, node, axis, query, interned_text),
        LayoutMode::HStack => {
            stack::intrinsic(engine, tree, node, Axis::X, axis, query, interned_text)
        }
        LayoutMode::VStack => {
            stack::intrinsic(engine, tree, node, Axis::Y, axis, query, interned_text)
        }
        LayoutMode::WrapHStack => {
            wrapstack::intrinsic(engine, tree, node, Axis::X, axis, query, interned_text)
        }
        LayoutMode::WrapVStack => {
            wrapstack::intrinsic(engine, tree, node, Axis::Y, axis, query, interned_text)
        }
        LayoutMode::ZStack => zstack::intrinsic(engine, tree, node, axis, query, interned_text),
        LayoutMode::Canvas => canvas::intrinsic(engine, tree, node, axis, query, interned_text),
        LayoutMode::Grid(grid_def_id) => {
            grid::intrinsic(engine, tree, node, grid_def_id, axis, query, interned_text)
        }
        // Scroll viewports "want" zero on every panned axis — sizing
        // comes from the viewport's own `Sizing`, never from content.
        // The non-panned axis falls back to a stack intrinsic on the
        // panned axis (pan-Y → stack on Y, pan-X → stack on X). If
        // both axes pan, the answer is unconditionally zero.
        LayoutMode::Scroll(scroll_spec) => {
            let pan = scroll_spec.pan_mask();
            let pan_axis = match axis {
                Axis::X => pan.x,
                Axis::Y => pan.y,
            };
            if pan_axis {
                IntrinsicRange::ZERO
            } else {
                let main = if pan.y { Axis::Y } else { Axis::X };
                stack::intrinsic(engine, tree, node, main, axis, query, interned_text)
            }
        }
    }
}

/// Leaf: walk shapes and aggregate. Only `ShapeRecord::Text` contributes
/// non-zero intrinsics today; other shapes are owner-relative paint and
/// don't drive size. Lives here rather than in a `leaf` module because
/// there isn't one — leaves have no driver, the leaf path is just "ask
/// the recorded shapes."
fn leaf<const RANGE: bool>(
    engine: &mut LayoutEngine,
    tree: &Tree,
    node: NodeId,
    axis: Axis,
    query: IntrinsicQuery<RANGE>,
    interned_text: &InternedText<'_>,
) -> IntrinsicRange {
    let wid = tree.records.widget_id()[node.idx()];
    let mut range = IntrinsicRange::ZERO;
    for ts in leaf_text_shapes(tree, interned_text, node) {
        let measurement = shape_leaf_text(engine, wid, &ts);
        if query.includes(LenReq::MinContent) {
            range.min = range
                .min
                .max(leaf_value(axis, LenReq::MinContent, ts.wrap, measurement));
        }
        if query.includes(LenReq::MaxContent) {
            range.max = range
                .max
                .max(leaf_value(axis, LenReq::MaxContent, ts.wrap, measurement));
        }
    }
    range
}

fn shape_leaf_text(
    engine: &mut LayoutEngine,
    wid: WidgetId,
    ts: &TextShapeInput<'_>,
) -> TextMeasurement {
    engine
        .text
        .prepare(
            TextRunIdentity {
                widget_id: wid,
                ordinal: ts.ordinal,
            },
            ts.shape_request(),
        )
        .unbounded
}

fn leaf_value(axis: Axis, req: LenReq, wrap: TextWrap, measurement: TextMeasurement) -> f32 {
    match (axis, req) {
        (Axis::X, LenReq::MinContent) => match wrap {
            TextWrap::WrapWithOverflow => measurement.intrinsic_min,
            TextWrap::SingleLine => measurement.size.w,
            TextWrap::Wrap | TextWrap::Truncate | TextWrap::Ellipsis | TextWrap::Scroll => 0.0,
        },
        (Axis::X, LenReq::MaxContent) => match wrap {
            TextWrap::Scroll => 0.0,
            _ => measurement.size.w,
        },
        (Axis::Y, _) => measurement.size.h,
    }
}

#[cfg(test)]
mod tests;
