//! Layout-side scroll driver. Measure records the content extent on [`PlaneLayout::scroll_content`](crate::layout::plane_layout::PlaneLayout::scroll_content); arrange delegates to the matching stack driver; intrinsic applies measure's per-axis contribution rule.

use crate::layout::drivers::LayoutDriver;
use crate::layout::drivers::stack::Stack;
use crate::layout::drivers::zstack::ZStack;
use crate::layout::intrinsic::intrinsic_pass::IntrinsicPass;
use crate::layout::intrinsic::intrinsic_query::IntrinsicQuery;
use crate::layout::intrinsic::intrinsic_range::IntrinsicRange;
use crate::layout::intrinsic::len_req::LenReq;
use crate::layout::measured::Measured;
use crate::layout::pass::LayoutPass;
use crate::primitives::geometry::size::Size;
use crate::primitives::layout::scroll_axes::{ScrollAxes, ScrollChildLayout};
use crate::scene::tree::node_id::NodeId;

#[derive(Clone, Copy, Debug)]
pub(super) struct Scroll {
    node: NodeId,
    /// The viewport's pan axes, child layout and fit rule.
    axes: ScrollAxes,
}

impl Scroll {
    pub(super) const fn new(node: NodeId, axes: ScrollAxes) -> Self {
        Self { node, axes }
    }
}

impl LayoutDriver for Scroll {
    const ARRANGE_DEPENDS_ONLY_ON_SLOT: bool = true;

    /// Measures children with unbounded space on panned axes, records their full content extent, and returns the viewport's desired size.
    fn measure(self, pass: &mut LayoutPass<'_>, inner_avail: Size) -> Measured {
        let Self { node, axes } = self;
        // A panned axis measures unbounded: what it scrolls over isn't limited by what it shows.
        let child_avail = Size::INF.select(axes.pan_mask(), inner_avail);
        let raw = match axes.child_layout() {
            ScrollChildLayout::Layered => ZStack::new(node).measure(pass, child_avail),
            ScrollChildLayout::Flow(main) => Stack::new(node, main).measure(pass, child_avail),
        };

        pass.set_scroll_content(node, raw.size);

        // A panned axis gives way whatever it shows, so it floors at zero (peer of its zero min-content).
        Measured {
            size: raw.size.select(axes.contributes_mask(), Size::ZERO),
            floor: Size::ZERO.select(axes.pan_mask(), raw.floor),
            stable_from: Size::ZERO.select(axes.pan_mask(), raw.stable_from),
        }
    }

    /// The child driver arranges with no give on the panned axes, where content takes what it measured.
    fn arrange(self, pass: &mut LayoutPass<'_>, inner: Size) {
        let Self { node, axes } = self;
        match axes.child_layout() {
            ScrollChildLayout::Layered => ZStack::new(node)
                .rigid_on(axes.pan_mask())
                .arrange(pass, inner),
            ScrollChildLayout::Flow(main) => Stack::new(node, main)
                .rigid_on(axes.pan_mask())
                .arrange(pass, inner),
        }
    }

    /// Must answer what measure would: same child driver and per-axis rule, both off the axes ([`ScrollAxes::contributes`] states the `fit` case).
    ///
    /// *Min*-content on a panned axis is zero (shrinking below the content is what scrolling is, and `AxisSlot::resolve` floors the viewport with it, so more would pin a `Hug` scroll open). *Max*-content is the content extent exactly when the author asked to `fit`.
    ///
    /// A half the caller didn't ask for is dropped; a query with neither skips the child walk.
    fn intrinsic(self, pass: &mut IntrinsicPass<'_>, query: IntrinsicQuery) -> IntrinsicRange {
        let Self { node, axes } = self;
        let axis = query.axis();
        let Some(content_query) = query.retain(|req| match req {
            LenReq::MinContent => !axes.pans(axis),
            LenReq::MaxContent => axes.contributes(axis),
        }) else {
            return IntrinsicRange::ZERO;
        };
        let content = match axes.child_layout() {
            ScrollChildLayout::Layered => ZStack::new(node).intrinsic(pass, content_query),
            ScrollChildLayout::Flow(main) => Stack::new(node, main).intrinsic(pass, content_query),
        };
        IntrinsicRange {
            min: if content_query.includes(LenReq::MinContent) {
                content.min
            } else {
                0.0
            },
            max: if content_query.includes(LenReq::MaxContent) {
                content.max
            } else {
                0.0
            },
        }
    }
}

#[cfg(test)]
mod tests;
