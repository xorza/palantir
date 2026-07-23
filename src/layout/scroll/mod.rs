//! Layout-side scroll driver. Measure records the content extent on
//! [`LayerLayout::scroll_content`]; arrange delegates child placement
//! to the matching stack driver.

use crate::layout::LayerLayout;
use crate::layout::axis::Axis;
use crate::layout::engine::LayoutEngine;
use crate::layout::stack;
use crate::layout::types::layout_mode::ScrollSpec;
use crate::layout::zstack;
use crate::primitives::interned_str::InternedText;
use crate::primitives::rect::Rect;
use crate::primitives::size::Size;
use crate::scene::tree::Tree;
use crate::scene::tree::node::NodeId;

/// Measures scroll children with unbounded space on the panned axes,
/// records their full content extent, and returns the viewport's
/// desired size.
#[profiling::function]
pub(crate) fn measure(
    layout: &mut LayoutEngine,
    tree: &Tree,
    node: NodeId,
    inner_avail: Size,
    spec: ScrollSpec,
    interned_text: &InternedText<'_>,
    out: &mut LayerLayout,
) -> Size {
    let pan = spec.pan_mask();
    let fit = spec.fit_mask();
    let child_avail = Size::new(
        if pan.x { f32::INFINITY } else { inner_avail.w },
        if pan.y { f32::INFINITY } else { inner_avail.h },
    );
    let raw = if pan.x && pan.y {
        zstack::measure(layout, tree, node, child_avail, interned_text, out)
    } else if pan.y {
        stack::measure(layout, tree, node, child_avail, Axis::Y, interned_text, out)
    } else {
        stack::measure(layout, tree, node, child_avail, Axis::X, interned_text, out)
    };

    out.scroll_content[node.idx()] = raw;

    Size::new(
        if pan.x && !fit.x { 0.0 } else { raw.w },
        if pan.y && !fit.y { 0.0 } else { raw.h },
    )
}

pub(crate) fn arrange(
    layout: &mut LayoutEngine,
    tree: &Tree,
    node: NodeId,
    inner: Rect,
    spec: ScrollSpec,
    out: &mut LayerLayout,
) {
    let pan = spec.pan_mask();
    if pan.x && pan.y {
        zstack::arrange(layout, tree, node, inner, out);
    } else if pan.y {
        stack::arrange(layout, tree, node, inner, Axis::Y, out);
    } else {
        stack::arrange(layout, tree, node, inner, Axis::X, out);
    }
}

#[cfg(test)]
mod tests;
