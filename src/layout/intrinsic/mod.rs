//! Intrinsic-dimensions queries — the on-demand `LenReq` API.
//!
//! This module owns:
//! - The query types: `LenReq`, `IntrinsicQuery`, and the ranges a walk
//!   answers with.
//! - The central `IntrinsicQuery::walk` dispatch that handles `Sizing`
//!   overrides, padding/margin, and `min_size`/`max_size` clamps before
//!   delegating to each driver's `intrinsic()` for content-driven sizes.
//! - Leaf intrinsics (no driver module owns leaves).
//!
//! Per-driver intrinsic logic lives alongside that driver's
//! `measure`/`arrange`, in its [`LayoutDriver`] impl — same
//! per-driver-file convention as the rest of layout.

pub(crate) mod intrinsic_query;
pub(crate) mod intrinsic_range;
pub(crate) mod intrinsic_walk;
pub(crate) mod len_req;

use crate::layout::axis::Axis;
use crate::layout::axis_slot::AxisSlot;
use crate::layout::driver::{DriverOp, LayoutDriver};
use crate::layout::engine::LayoutEngine;
use crate::layout::intrinsic::intrinsic_query::IntrinsicQuery;
use crate::layout::intrinsic::intrinsic_range::IntrinsicRange;
use crate::layout::intrinsic::intrinsic_walk::IntrinsicWalk;
use crate::layout::intrinsic::len_req::LenReq;
use crate::layout::text_shape_input::TextShapeInput;
use crate::primitives::interned_text::InternedText;
use crate::primitives::size::Size;
use crate::scene::node::bounds_extras::BoundsExtras;
use crate::scene::node::layout_core::LayoutCore;
use crate::scene::tree::Tree;
use crate::scene::tree::node_id::NodeId;
use crate::text::system::TextRunSlot;

/// Wrap a raw content range in the node's own box on `axis`: padding, the
/// `Sizing` override, margin, and the `min_size` / `max_size` clamps.
///
/// Padding is added unconditionally. A Fixed axis arrives with a zero
/// content range, and `AxisSlot::resolve` returns the declared value
/// without reading either — so the add cannot reach the result.
fn outer(
    layout: LayoutCore,
    bounds: &BoundsExtras,
    axis: Axis,
    query: IntrinsicQuery,
    mut content: IntrinsicRange,
) -> IntrinsicRange {
    let pad = axis.spacing(layout.padding);
    let slot = AxisSlot {
        sizing: axis.main_sizing(layout.size),
        available: f32::INFINITY,
        intrinsic_min: 0.0,
        margin: axis.spacing(layout.margin),
        min: axis.main(bounds.min_size),
        max: axis.main(bounds.max_size),
    };
    for (_, value) in content.requested(query) {
        *value = slot.resolve(*value + pad);
    }
    content
}

/// The only [`DriverOp`] of the three that carries no `LayoutPass`. That is the
/// point: a pure query of a subtree must not be able to write the frame's
/// text shapes, and holding the engine and the tree separately is what
/// keeps a `LayerLayout` out of reach.
#[derive(Debug)]
struct IntrinsicOp<'op, 'text> {
    engine: &'op mut LayoutEngine,
    tree: &'op Tree,
    node: NodeId,
    axis: Axis,
    query: IntrinsicQuery,
    interned_text: &'op InternedText<'text>,
}

impl DriverOp for IntrinsicOp<'_, '_> {
    type Output = IntrinsicWalk;

    fn run<D: LayoutDriver>(self, payload: D::Payload) -> IntrinsicWalk {
        let Self {
            engine,
            tree,
            node,
            axis,
            query,
            interned_text,
        } = self;
        IntrinsicWalk::one_axis(D::intrinsic(
            engine,
            tree,
            node,
            payload,
            axis,
            query,
            interned_text,
        ))
    }

    fn leaf(self) -> IntrinsicWalk {
        let Self {
            engine,
            tree,
            node,
            axis,
            query,
            interned_text,
        } = self;
        leaf(engine, tree, node, axis, query, interned_text)
    }
}

/// Leaf: walk shapes and aggregate. Only `ShapeRecord::Text` contributes
/// non-zero intrinsics today; other shapes are owner-relative paint and
/// don't drive size. Lives here rather than in a `leaf` module because
/// there isn't one — leaves have no driver, the leaf path is just "ask
/// the recorded shapes."
///
/// A run's content demands are `Size`s, so the accumulators are too and
/// both axes fall out of the same pass. `axis` picks the answered lane
/// at the end; see [`IntrinsicWalk`] for what the other one buys.
fn leaf(
    engine: &mut LayoutEngine,
    tree: &Tree,
    node: NodeId,
    axis: Axis,
    query: IntrinsicQuery,
    interned_text: &InternedText<'_>,
) -> IntrinsicWalk {
    let wid = tree.records.widget_id()[node.idx()];
    let mut min_content = Size::ZERO;
    let mut max_content = Size::ZERO;
    for ts in TextShapeInput::on_leaf(tree, interned_text, node) {
        let unbounded = engine.text.root(
            TextRunSlot {
                widget_id: wid,
                ordinal: ts.ordinal,
            },
            ts.shape_request(),
            ts.wrap,
        );
        if query.includes(LenReq::MinContent) {
            min_content = min_content.max(ts.wrap.min_content(&unbounded));
        }
        if query.includes(LenReq::MaxContent) {
            max_content = max_content.max(ts.wrap.max_content(&unbounded));
        }
    }
    IntrinsicWalk {
        answered: IntrinsicRange {
            min: axis.main(min_content),
            max: axis.main(max_content),
        },
        sibling: Some(IntrinsicRange {
            min: axis.cross(min_content),
            max: axis.cross(max_content),
        }),
    }
}

#[cfg(test)]
mod tests;
