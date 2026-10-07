//! The absolute-position driver: children sit at their own offsets; the container reports the extent covering them.

use crate::layout::axis_placement::AxisPlacement;
use crate::layout::drivers::LayoutDriver;
use crate::layout::engine::LayoutEngine;
use crate::layout::intrinsic::intrinsic_query::IntrinsicQuery;
use crate::layout::intrinsic::intrinsic_range::IntrinsicRange;
use crate::layout::measured::Measured;
use crate::layout::pass::LayoutPass;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::geometry::size::Size;
use crate::primitives::layout::axis::Axis;
use crate::primitives::text::interned_text::InternedText;
use crate::scene::tree::Tree;
use crate::scene::tree::node_id::NodeId;

#[derive(Debug)]
pub(super) struct Canvas;

impl LayoutDriver for Canvas {
    type Payload = ();

    const ARRANGE_DEPENDS_ONLY_ON_SLOT: bool = true;

    /// Canvas: children placed at their declared `Layout.position` (parent-inner coords, default `(0, 0)`), each measured against the room past where it sits; see [`LayoutPass::measure_per_axis_hug`].
    ///
    /// **Content size per axis depends on the canvas's sizing on that axis.** A `Hug` axis reports `max(child_pos + child_desired)`; a `Fill` axis reports `max(child_desired)`, so `.position(...)` cannot inflate the canvas past its available and flicker `Damage::Full` while dragging.
    /// Negative positions render outside `inner` either way (the running max starts at 0); build scrollable negative-origin canvases on [`crate::widgets::scroll::Scroll`].
    fn measure(
        pass: &mut LayoutPass<'_>,
        node: NodeId,
        (): Self::Payload,
        inner_avail: Size,
    ) -> Measured {
        // Active children only: a collapsed child must not inflate the content size. The position decides both the room a bounded axis has left and the extent a Hug axis grows to; `measure_per_axis_hug` derives each.
        pass.measure_per_axis_hug(node, inner_avail, |tree, c| tree.bounds(c).position)
    }

    /// Each child gets a slot at `bounds.position`, sized per its desired size; `Fill` falls back to intrinsic as in `measure`.
    fn arrange(pass: &mut LayoutPass<'_>, node: NodeId, (): Self::Payload, inner: Size) {
        let tree = pass.tree;
        let layouts = tree.records.layout();
        let canvas_size = layouts[node.idx()].size;
        for child in tree.children(node) {
            let c = child.id;
            let d = pass.desired(c);
            let child_layout = layouts[c.idx()];
            let bounds = tree.bounds(c);
            let pos = bounds.position;
            // A bounded axis gives the room past the child's position, exactly what `measure` offered, so a wrapping child arranges at the width it shaped against. A Hug axis gives the desired.
            let room = inner.room_past(pos);
            let slot = d.select(canvas_size.hug_mask(), room);
            let child_rect = Rect {
                min: pos,
                size: AxisPlacement::arrange_size(&child_layout, bounds, pass.placed(c), slot),
            };
            pass.arrange(c, child_rect);
        }
    }

    /// Intrinsic size of a Canvas, mirroring `measure`: `Hug` returns `max(child.position + child.intrinsic)`; `Fill` drops the offset so a `.position(...)` past `available` cannot floor it.
    fn intrinsic(
        layout: &mut LayoutEngine,
        tree: &Tree,
        node: NodeId,
        (): Self::Payload,
        axis: Axis,
        query: IntrinsicQuery,
        interned_text: &InternedText<'_>,
    ) -> IntrinsicRange {
        let pos_inflates = axis
            .main_sizing(tree.records.layout()[node.idx()].size)
            .is_hug();
        query.children_max(layout, tree, node, axis, interned_text, |tree, c| {
            if pos_inflates {
                axis.main_v(tree.bounds(c).position)
            } else {
                0.0
            }
        })
    }
}

#[cfg(test)]
mod tests;
