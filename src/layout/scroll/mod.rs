//! Layout-side scroll driver. Measure records the content extent on
//! [`LayerLayout::scroll_content`](crate::layout::LayerLayout::scroll_content);
//! arrange delegates child placement to the matching stack driver, and
//! intrinsic answers the same per-axis contribution rule measure does.

use crate::layout::axis::Axis;
use crate::layout::engine::LayoutEngine;
use crate::layout::intrinsic::{IntrinsicQuery, IntrinsicRange, LenReq};
use crate::layout::pass::LayoutPass;
use crate::layout::stack;
use crate::layout::types::layout_mode::{ScrollChildLayout, ScrollSpec};
use crate::layout::zstack;
use crate::primitives::interned_text::InternedText;
use crate::primitives::rect::Rect;
use crate::primitives::size::Size;
use crate::scene::tree::Tree;
use crate::scene::tree::node_id::NodeId;

/// Measures scroll children with unbounded space on the panned axes,
/// records their full content extent, and returns the viewport's
/// desired size.
pub(super) fn measure(
    pass: &mut LayoutPass<'_>,
    node: NodeId,
    inner_avail: Size,
    spec: ScrollSpec,
) -> Size {
    let pan = spec.pan_mask();
    let child_avail = Size::new(
        if pan.x { f32::INFINITY } else { inner_avail.w },
        if pan.y { f32::INFINITY } else { inner_avail.h },
    );
    let raw = match spec.child_layout() {
        ScrollChildLayout::Layered => zstack::measure(pass, node, child_avail),
        ScrollChildLayout::Flow(main) => stack::measure(pass, node, child_avail, main),
    };

    pass.set_scroll_content(node, raw);

    Size::new(
        if spec.contributes(Axis::X) {
            raw.w
        } else {
            0.0
        },
        if spec.contributes(Axis::Y) {
            raw.h
        } else {
            0.0
        },
    )
}

pub(super) fn arrange(pass: &mut LayoutPass<'_>, node: NodeId, inner: Rect, spec: ScrollSpec) {
    match spec.child_layout() {
        ScrollChildLayout::Layered => zstack::arrange(pass, node, inner),
        ScrollChildLayout::Flow(main) => stack::arrange(pass, node, inner, main),
    }
}

/// A scroll's intrinsic has to answer exactly what its measure would: same
/// child driver, same per-axis contribution rule. Both come off the spec so
/// the two can't drift — [`ScrollSpec::contributes`] is where the `fit` case
/// is stated.
///
/// **A scroll's two content sizes differ in kind, so one rule can't serve
/// both.** *Min*-content on a panned axis is zero: being able to shrink
/// below the content is what scrolling *is*, and `resolve_sizing` floors the
/// viewport's own size with this, so anything larger pins a `Hug` scroll open
/// at its content. *Max*-content is what the viewport would take given room
/// — the content extent exactly when the author asked it to `fit`.
///
/// Either half the caller did not ask for is dropped, and a query left with
/// neither skips the child walk entirely.
pub(super) fn intrinsic(
    engine: &mut LayoutEngine,
    tree: &Tree,
    node: NodeId,
    spec: ScrollSpec,
    axis: Axis,
    query: IntrinsicQuery,
    interned_text: &InternedText<'_>,
) -> IntrinsicRange {
    let wants_min = query.includes(LenReq::MinContent) && !spec.pans(axis);
    let wants_max = query.includes(LenReq::MaxContent) && spec.contributes(axis);
    let Some(content_query) = IntrinsicQuery::of(wants_min, wants_max) else {
        return IntrinsicRange::ZERO;
    };
    let content = match spec.child_layout() {
        ScrollChildLayout::Layered => {
            zstack::intrinsic(engine, tree, node, axis, content_query, interned_text)
        }
        ScrollChildLayout::Flow(main) => {
            stack::intrinsic(engine, tree, node, main, axis, content_query, interned_text)
        }
    };
    IntrinsicRange {
        min: if wants_min { content.min } else { 0.0 },
        max: if wants_max { content.max } else { 0.0 },
    }
}

#[cfg(test)]
mod tests;
