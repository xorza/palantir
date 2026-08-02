//! Layout-side scroll driver. Measure records the content extent on
//! [`LayerLayout::scroll_content`]; arrange delegates child placement
//! to the matching stack driver.

use crate::layout::axis::Axis;
use crate::layout::pass::LayoutPass;
use crate::layout::stack;
use crate::layout::types::layout_mode::ScrollSpec;
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
    let fit = spec.fit_mask();
    let child_avail = Size::new(
        if pan.x { f32::INFINITY } else { inner_avail.w },
        if pan.y { f32::INFINITY } else { inner_avail.h },
    );
    let raw = if pan.x && pan.y {
        zstack::measure(pass, node, child_avail)
    } else if pan.y {
        stack::measure(pass, node, child_avail, Axis::Y)
    } else {
        stack::measure(pass, node, child_avail, Axis::X)
    };

    pass.set_scroll_content(node, raw);

    Size::new(
        if pan.x && !fit.x { 0.0 } else { raw.w },
        if pan.y && !fit.y { 0.0 } else { raw.h },
    )
}

pub(super) fn arrange(pass: &mut LayoutPass<'_>, node: NodeId, inner: Rect, spec: ScrollSpec) {
    let pan = spec.pan_mask();
    if pan.x && pan.y {
        zstack::arrange(pass, node, inner);
    } else if pan.y {
        stack::arrange(pass, node, inner, Axis::Y);
    } else {
        stack::arrange(pass, node, inner, Axis::X);
    }
}

#[cfg(test)]
mod tests;
