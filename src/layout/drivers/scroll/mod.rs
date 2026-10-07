//! Layout-side scroll driver. Measure records the content extent on [`LayerLayout::scroll_content`](crate::layout::layer_layout::LayerLayout::scroll_content); arrange delegates to the matching stack driver; intrinsic applies measure's per-axis contribution rule.

use crate::layout::drivers::LayoutDriver;
use crate::layout::drivers::stack::Stack;
use crate::layout::drivers::zstack::ZStack;
use crate::layout::engine::LayoutEngine;
use crate::layout::intrinsic::intrinsic_query::IntrinsicQuery;
use crate::layout::intrinsic::intrinsic_range::IntrinsicRange;
use crate::layout::intrinsic::len_req::LenReq;
use crate::layout::measured::Measured;
use crate::layout::pass::LayoutPass;
use crate::primitives::layout::axis::Axis;
use crate::primitives::layout::scroll_axes::{ScrollAxes, ScrollChildLayout};
use crate::primitives::text::interned_text::InternedText;

use crate::primitives::geometry::size::Size;
use crate::scene::tree::Tree;
use crate::scene::tree::node_id::NodeId;

#[derive(Debug)]
pub(super) struct Scroll;

impl LayoutDriver for Scroll {
    /// The viewport's pan axes, child layout and fit rule.
    type Payload = ScrollAxes;

    const ARRANGE_DEPENDS_ONLY_ON_SLOT: bool = true;

    /// Measures children with unbounded space on panned axes, records their full content extent, and returns the viewport's desired size.
    fn measure(
        pass: &mut LayoutPass<'_>,
        node: NodeId,
        axes: Self::Payload,
        inner_avail: Size,
    ) -> Measured {
        // A panned axis measures unbounded: what it scrolls over isn't limited by what it shows.
        let child_avail = Size::INF.select(axes.pan_mask(), inner_avail);
        let raw = match axes.child_layout() {
            ScrollChildLayout::Layered => ZStack::measure(pass, node, (), child_avail),
            ScrollChildLayout::Flow(main) => Stack::measure(pass, node, main, child_avail),
        };

        pass.set_scroll_content(node, raw.size);

        // A panned axis gives way whatever it shows, so it floors at zero (peer of its zero min-content).
        Measured {
            size: raw.size.select(axes.contributes_mask(), Size::ZERO),
            floor: Size::ZERO.select(axes.pan_mask(), raw.floor),
            stable_from: Size::ZERO.select(axes.pan_mask(), raw.stable_from),
        }
    }

    fn arrange(pass: &mut LayoutPass<'_>, node: NodeId, axes: Self::Payload, inner: Size) {
        match axes.child_layout() {
            ScrollChildLayout::Layered => ZStack::arrange_in(pass, node, inner, axes.pan_mask()),
            ScrollChildLayout::Flow(main) => {
                Stack::arrange_in(pass, node, main, inner, axes.pan_mask());
            }
        }
    }

    /// Must answer what measure would: same child driver and per-axis rule, both off the payload ([`ScrollAxes::contributes`] states the `fit` case).
    ///
    /// *Min*-content on a panned axis is zero (shrinking below the content is what scrolling is, and `AxisSlot::resolve` floors the viewport with it, so more would pin a `Hug` scroll open). *Max*-content is the content extent exactly when the author asked to `fit`.
    ///
    /// A half the caller didn't ask for is dropped; a query with neither skips the child walk.
    fn intrinsic(
        engine: &mut LayoutEngine,
        tree: &Tree,
        node: NodeId,
        axes: Self::Payload,
        axis: Axis,
        query: IntrinsicQuery,
        interned_text: &InternedText<'_>,
    ) -> IntrinsicRange {
        let wants_min = query.includes(LenReq::MinContent) && !axes.pans(axis);
        let wants_max = query.includes(LenReq::MaxContent) && axes.contributes(axis);
        let Some(content_query) = IntrinsicQuery::of(wants_min, wants_max) else {
            return IntrinsicRange::ZERO;
        };
        let content = match axes.child_layout() {
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
