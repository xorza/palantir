//! Layout-side scroll subsystem. Owns:
//!
//! - **Driver** ([`measure`] + [`arrange`]) — minimal: INF-axis
//!   measure, standard arrange, records the content extent into
//!   [`ScrollContent`].
//! - **Cross-frame state** ([`ScrollLayoutState`]) — offset,
//!   viewport, outer, content, overflow, seen — keyed by `WidgetId`
//!   on [`LayoutEngine::scroll_states`]. The scroll widget reads and
//!   mutates this directly; refresh updates the layout-derived
//!   fields.
//! - **Refresh** ([`LayoutEngine::refresh_scrolls`]) — runs after
//!   arrange. Walks each layer's tree for `LayoutMode::Scroll` nodes
//!   and updates the matching state row.
//!
//! Bar-gutter reservation and bar drawing live in
//! [`crate::widgets::scroll`] — the layout primitive itself is
//! unaware of scrollbars.

use crate::forest::element::ScrollAxes;
use crate::forest::tree::{Layer, NodeId, Tree};
use crate::forest::widget_id::WidgetId;
use crate::layout::axis::Axis;
use crate::primitives::rect::Rect;
use crate::primitives::size::Size;
use crate::text::TextShaper;
use glam::Vec2;
use rustc_hash::FxHashMap;
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
/// [`measure`]. Read by [`LayoutEngine::refresh_scrolls`] after
/// arrange to update each scroll widget's [`ScrollLayoutState`].
///
/// Sparse — only Scroll nodes appear. On a measure-cache hit
/// [`measure`] doesn't fire and no entry is pushed; refresh falls
/// back to the persisted state row's `content` (cache-hit ⟹
/// identical measure ⟹ last frame's value is right).
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
// Cross-frame state — what the scroll widget reads at record time
// ---------------------------------------------------------------------------

/// Cross-frame state row for one scroll widget. Owned by
/// [`LayoutEngine::scroll_states`] — *not* `StateMap`, because this
/// is a layout-derived concern (refresh writes layout fields) and
/// belongs in the layout subsystem rather than tangled with widget
/// state.
///
/// The widget at record time:
/// - Reads the snapshot via [`Ui::scroll_state`](crate::Ui::scroll_state)
///   for offset clamp + reservation guess + bar geometry.
/// - Mutates `offset` from input (via direct entry access on the
///   layout's hashmap, or a helper).
///
/// `refresh` writes layout-derived fields (viewport, outer, content,
/// overflow, seen) and re-clamps `offset` to the new bounds.
///
/// - `offset` — input-accumulated pan position (next frame's start).
/// - `viewport` — INNER (user-padding-deflated) size: what children
///   see. Drives `content > viewport` overflow checks.
/// - `outer` — full arranged rect size including any reserved bar
///   strips. Drives bar positioning so the bar sits flush with the
///   OUTER far edge.
/// - `content` — measured content extent on the panned axes.
/// - `overflow` — `(x, y)` per-axis: did this axis's content overflow
///   the viewport on the most recent measure? Read at record time
///   to decide whether to reserve a bar gutter on the cross axis.
/// - `seen` — set true by `refresh` after the first frame. Read by
///   the widget to detect a cold-mount and trigger a relayout pass
///   so pass B records with the measured reservation in place.
#[derive(Default, Clone, Copy, Debug)]
pub(crate) struct ScrollLayoutState {
    pub(crate) offset: Vec2,
    pub(crate) viewport: Size,
    pub(crate) outer: Size,
    pub(crate) content: Size,
    pub(crate) overflow: (bool, bool),
    pub(crate) seen: bool,
}

/// Cross-frame map of [`ScrollLayoutState`] keyed by `WidgetId`.
/// Lives on [`LayoutEngine`]; refresh writes layout-derived fields,
/// the widget mutates `offset` from input.
pub(crate) type ScrollStates = FxHashMap<WidgetId, ScrollLayoutState>;

// ---------------------------------------------------------------------------
// Measure / arrange dispatch
// ---------------------------------------------------------------------------

/// Measure dispatch arm for [`LayoutMode::Scroll`]. Single
/// child-measurement pass with `INF` on the panned axes — no
/// reservation, no awareness of bars. Returns the panned-axis-zeroed
/// `desired` so the viewport's own size doesn't grow with content.
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

/// Arrange dispatch arm for [`LayoutMode::Scroll`]. Plain delegate to
/// stack/zstack arrange — children land in `inner` (already deflated
/// by user padding). Bar-gutter reservation, if any, was applied as
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
