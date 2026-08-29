//! Layout-side scroll driver. Measure records the content extent on
//! [`LayerLayout::scroll_content`](crate::layout::LayerLayout::scroll_content);
//! arrange delegates child placement to the matching stack driver, and
//! intrinsic answers the same per-axis contribution rule measure does.

use crate::layout::axis::Axis;
use crate::layout::driver::LayoutDriver;
use crate::layout::engine::LayoutEngine;
use crate::layout::intrinsic::{IntrinsicQuery, IntrinsicRange, LenReq};
use crate::layout::pass::LayoutPass;
use crate::layout::stack::Stack;
use crate::layout::types::layout_mode::{ScrollChildLayout, ScrollSpec};
use crate::layout::zstack::ZStack;
use crate::primitives::interned_text::InternedText;
use crate::primitives::rect::Rect;
use crate::primitives::size::Size;
use crate::scene::tree::Tree;
use crate::scene::tree::node_id::NodeId;

#[derive(Debug)]
pub(super) struct Scroll;

impl LayoutDriver for Scroll {
    /// The viewport's pan axes, child layout and fit rule.
    type Payload = ScrollSpec;

    const ARRANGE_DEPENDS_ONLY_ON_SLOT: bool = true;

    /// Measures scroll children with unbounded space on the panned axes,
    /// records their full content extent, and returns the viewport's
    /// desired size.
    fn measure(
        pass: &mut LayoutPass<'_>,
        node: NodeId,
        spec: Self::Payload,
        inner_avail: Size,
    ) -> Size {
        // A panned axis measures unbounded: what it scrolls over is not
        // limited by what it shows.
        let child_avail = Size::INF.select(spec.pan_mask(), inner_avail);
        let raw = match spec.child_layout() {
            ScrollChildLayout::Layered => ZStack::measure(pass, node, (), child_avail),
            ScrollChildLayout::Flow(main) => Stack::measure(pass, node, main, child_avail),
        };

        pass.set_scroll_content(node, raw);

        raw.select(spec.contributes_mask(), Size::ZERO)
    }

    fn arrange(pass: &mut LayoutPass<'_>, node: NodeId, spec: Self::Payload, inner: Rect) {
        match spec.child_layout() {
            ScrollChildLayout::Layered => ZStack::arrange(pass, node, (), inner),
            ScrollChildLayout::Flow(main) => Stack::arrange(pass, node, main, inner),
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
    fn intrinsic(
        engine: &mut LayoutEngine,
        tree: &Tree,
        node: NodeId,
        spec: Self::Payload,
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
                ZStack::intrinsic(engine, tree, node, (), axis, content_query, interned_text)
            }
            ScrollChildLayout::Flow(main) => {
                Stack::intrinsic(engine, tree, node, main, axis, content_query, interned_text)
            }
        };
        IntrinsicRange {
            min: if wants_min { content.min } else { 0.0 },
            max: if wants_max { content.max } else { 0.0 },
        }
    }
}

#[cfg(test)]
mod tests;
