//! Layout-side scroll driver. Measure records the content extent on
//! [`LayerLayout::scroll_content`]; arrange delegates child placement
//! to the matching stack driver.

use crate::layout::axis::Axis;
use crate::layout::pass::LayoutPass;
use crate::layout::stack;
use crate::layout::types::layout_mode::{ScrollChildLayout, ScrollSpec};
use crate::layout::zstack;
use crate::primitives::rect::Rect;
use crate::primitives::size::Size;
use crate::scene::tree::record::NodeId;

/// Measures scroll children with unbounded space on the panned axes,
/// records their full content extent, and returns the viewport's
/// desired size.
#[profiling::function]
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

#[cfg(test)]
mod tests;
