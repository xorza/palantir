//! Minimal layout driver for
//! [`crate::forest::element::LayoutMode::Scroll`] viewports. Just
//! "INF-axis measure + standard arrange + records the content extent
//! so the widget can read it post-layout." Bar-gutter reservation,
//! bar drawing, and `ScrollState` updates live in
//! [`crate::widgets::scroll`] — the layout primitive is unaware of
//! scrollbars.
//!
//! Output: one [`(NodeId, Size)`] entry per Scroll node in
//! [`LayoutEngine::scroll_content`] for the active layer. Read by
//! `widgets::scroll::refresh` after arrange to update each widget's
//! `ScrollState`.

use crate::forest::element::ScrollAxes;
use crate::forest::tree::{Layer, NodeId, Tree};
use crate::layout::axis::Axis;
use crate::primitives::rect::Rect;
use crate::primitives::size::Size;
use crate::text::TextShaper;
use strum::EnumCount as _;

use super::LayoutEngine;
use super::stack;
use super::zstack;

#[cfg(test)]
mod tests;

// ---------------------------------------------------------------------------
// Per-frame content extents
// ---------------------------------------------------------------------------

/// Per-layer flat Vec of `(inner_node, content_size)` pairs pushed by
/// [`measure`]. Read by `widgets::scroll::refresh` after arrange to
/// update each scroll widget's `ScrollState.content`.
///
/// Sparse — only Scroll nodes appear. On a measure-cache hit
/// [`measure`] doesn't fire and no entry is pushed; refresh falls
/// back to the persisted `ScrollState.content` (cache-hit ⟹ identical
/// measure ⟹ last frame's value is right).
#[derive(Default)]
pub(crate) struct ScrollContent {
    layers: [Vec<(NodeId, Size)>; Layer::COUNT],
}

impl ScrollContent {
    pub(crate) fn clear(&mut self) {
        for v in &mut self.layers {
            v.clear();
        }
    }

    pub(crate) fn for_layer(&self, layer: Layer) -> &[(NodeId, Size)] {
        &self.layers[layer as usize]
    }

    fn for_layer_mut(&mut self, layer: Layer) -> &mut Vec<(NodeId, Size)> {
        &mut self.layers[layer as usize]
    }

    /// Linear scan over a per-layer slice. Few scrolls per frame, so
    /// the scan is cheap.
    pub(crate) fn lookup(&self, layer: Layer, node: NodeId) -> Option<Size> {
        self.for_layer(layer)
            .iter()
            .find(|(n, _)| *n == node)
            .map(|(_, s)| *s)
    }
}

// ---------------------------------------------------------------------------
// Measure / arrange dispatch
// ---------------------------------------------------------------------------

/// Measure dispatch arm for [`crate::forest::element::LayoutMode::Scroll`].
/// Single child-measurement pass with `INF` on the panned axes — no
/// reservation, no awareness of bars (the widget owns those at record
/// time). Returns the panned-axis-zeroed `desired` so the viewport's
/// own size doesn't grow with content.
pub(crate) fn measure(
    engine: &mut LayoutEngine,
    tree: &Tree,
    node: NodeId,
    inner_avail: Size,
    axes: ScrollAxes,
    text: &TextShaper,
) -> Size {
    let raw = match axes {
        ScrollAxes::Vertical => stack::measure(
            engine,
            tree,
            node,
            Size::new(inner_avail.w, f32::INFINITY),
            Axis::Y,
            text,
        ),
        ScrollAxes::Horizontal => stack::measure(
            engine,
            tree,
            node,
            Size::new(f32::INFINITY, inner_avail.h),
            Axis::X,
            text,
        ),
        ScrollAxes::Both => zstack::measure(engine, tree, node, Size::INF, text),
    };

    engine
        .scroll_content
        .for_layer_mut(engine.active_layer)
        .push((node, raw));

    match axes {
        ScrollAxes::Vertical => Size::new(raw.w, 0.0),
        ScrollAxes::Horizontal => Size::new(0.0, raw.h),
        ScrollAxes::Both => Size::ZERO,
    }
}

/// Arrange dispatch arm for [`crate::forest::element::LayoutMode::Scroll`].
/// Plain delegate to stack/zstack arrange — children land in `inner`
/// (which the parent dispatcher already deflated by user padding).
/// Bar-gutter reservation, if any, was already applied as
/// `Element.padding` on the *outer* ZStack the widget builds.
pub(crate) fn arrange(
    engine: &mut LayoutEngine,
    tree: &Tree,
    node: NodeId,
    inner: Rect,
    axes: ScrollAxes,
) {
    match axes {
        ScrollAxes::Vertical => stack::arrange(engine, tree, node, inner, Axis::Y),
        ScrollAxes::Horizontal => stack::arrange(engine, tree, node, inner, Axis::X),
        ScrollAxes::Both => zstack::arrange(engine, tree, node, inner),
    }
}
