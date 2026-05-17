//! Layout-side scroll subsystem. Owns:
//!
//! - **Driver** ([`measure`] + [`arrange`]) — minimal: INF-axis
//!   measure that records the content extent into the matching
//!   [`ScrollLayoutState`] row, and a standard arrange that updates
//!   the layout-derived fields (viewport, overflow, seen) and
//!   re-clamps `offset` post-arrange. No separate post-pass — the
//!   driver is the refresh.
//! - **Cross-frame state** ([`ScrollLayoutState`]) — offset, viewport,
//!   content, overflow, seen — keyed by the inner viewport node's
//!   `WidgetId` on [`LayoutEngine::scroll_states`]. The scroll widget
//!   reads and mutates this directly (via [`Ui::scroll_state`], which
//!   applies the same `.with("__viewport")` hop transparently).
//!
//! Bar-gutter reservation and bar drawing live in
//! [`crate::widgets::scroll`] — the layout primitive itself is
//! unaware of scrollbars and of the outer ZStack the widget wraps it
//! in.

use crate::forest::element::LayoutMode;
use crate::forest::tree::{NodeId, Tree};
use crate::layout::Layout;
use crate::layout::axis::Axis;
use crate::primitives::rect::Rect;
use crate::primitives::size::Size;
use crate::primitives::spacing::Spacing;
use crate::primitives::widget_id::WidgetId;
use glam::Vec2;
use rustc_hash::FxHashMap;

use super::layoutengine::LayoutEngine;
use super::stack;
use super::support::TextCtx;
use super::zstack;

#[cfg(test)]
mod tests;

// ---------------------------------------------------------------------------
// Cross-frame state — what the scroll widget reads at record time
// ---------------------------------------------------------------------------

/// Cross-frame state row for one scroll widget. Owned by
/// [`LayoutEngine::scroll_states`] — *not* `StateMap`, because this
/// is a layout-derived concern (the driver writes layout fields) and
/// belongs in the layout subsystem rather than tangled with widget
/// state.
///
/// The widget at record time:
/// - Reads the snapshot via [`Ui::scroll_state`](crate::Ui::scroll_state)
///   for offset clamp + reservation guess + bar geometry.
/// - Mutates `offset` from input (via the same entry).
///
/// The driver writes layout-derived fields:
/// - `measure` records `content` (the panned-axis extent).
/// - `arrange` records `viewport` (inner rect post user-padding),
///   `overflow`, `seen`, and re-clamps `offset` to the new bounds.
///
/// - `offset` — input-accumulated pan position (next frame's start).
/// - `viewport` — INNER (user-padding-deflated) size: what children
///   see. Drives `content > viewport` overflow checks.
/// - `outer` — full arranged rect size of the scroll node including
///   any reservation gutter. Drives bar positioning so the bar sits
///   flush with the OUTER far edge. Parent-allocated and stable
///   across reservation flips (unlike `viewport`, which shrinks when
///   a gutter appears) — that's why we store it instead of deriving
///   from `viewport + padding + reservation` at record time.
/// - `content` — measured content extent on the panned axes.
/// - `overflow` — `(x, y)` per-axis: did this axis's content overflow
///   the viewport on the most recent measure? Read at record time
///   to decide whether to reserve a bar gutter on the cross axis.
/// - `seen` — set true by `arrange` after the first frame. Read by
///   the widget to detect a cold-mount and trigger a relayout pass
///   so pass B records with the measured reservation in place.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ScrollLayoutState {
    pub(crate) offset: Vec2,
    /// Uniform zoom; `1.0` = no zoom. Mutated only by [`Scroll`] widgets
    /// configured via `with_zoom*`. The driver leaves it alone.
    pub(crate) zoom: f32,
    pub(crate) viewport: Size,
    pub(crate) outer: Size,
    /// Unscaled content rect in scroll-content coords. `content.min`
    /// (≤ `(0,0)`) is the leading-edge offset rolled up from
    /// `LayoutScratch::content_origin`; `content.size` is the
    /// `bbox.max - bbox.min` extent the driver measured. Multiply
    /// by `zoom` for the user-perceived (post-paint) coordinates.
    /// Margin lives separately on `content_margin`.
    pub(crate) content: Rect,
    pub(crate) overflow: (bool, bool),
    pub(crate) seen: bool,
    /// Extra slack added around the measured content rect at clamp
    /// time. Doesn't touch child layout and doesn't show up in
    /// `content` — bars reflect the real bbox; the margin is invisible
    /// overscroll the user can pan into. Set by
    /// [`Scroll::content_margin`] each record. Used by canvas-style
    /// scopes (node graphs, infinite boards).
    pub(crate) content_margin: Spacing,
    /// Snapshot of `offset` at the frame a thumb-drag latched, paired
    /// with the axis whose thumb is being dragged. `Some` while a drag
    /// is in flight; cleared when the dragged thumb is no longer the
    /// active capture. Reused each frame so cumulative `drag_delta`
    /// composes against a stable anchor (otherwise per-frame deltas
    /// would compound).
    pub(crate) drag_anchor: Option<(Axis, Vec2)>,
}

impl Default for ScrollLayoutState {
    fn default() -> Self {
        Self {
            offset: Vec2::ZERO,
            zoom: 1.0,
            viewport: Size::ZERO,
            outer: Size::ZERO,
            content: Rect::ZERO,
            overflow: (false, false),
            seen: false,
            drag_anchor: None,
            content_margin: Spacing::default(),
        }
    }
}

/// Cross-frame map of [`ScrollLayoutState`] keyed by the inner
/// viewport's `WidgetId`. Lives on [`LayoutEngine`]; the driver
/// writes layout-derived fields, the widget mutates `offset` from
/// input.
pub(crate) type ScrollStates = FxHashMap<WidgetId, ScrollLayoutState>;

// ---------------------------------------------------------------------------
// Measure / arrange dispatch
// ---------------------------------------------------------------------------

/// Measure dispatch arm for [`LayoutMode::Scroll`]. Single
/// child-measurement pass with `INF` on the panned axes — no
/// reservation, no awareness of bars. Records the panned-axis content
/// extent into the persistent state row, and returns the
/// panned-axis-zeroed `desired` so the viewport's own size doesn't
/// grow with content.
///
/// On a measure-cache hit at any ancestor, this function doesn't run
/// and the row's `content` keeps last frame's value (cache hit ⟹
/// identical measure ⟹ identical content extent).
#[profiling::function]
pub(crate) fn measure(
    layout: &mut LayoutEngine,
    tree: &Tree,
    node: NodeId,
    inner_avail: Size,
    mode: LayoutMode,
    tc: &TextCtx<'_>,
    out: &mut Layout,
) -> Size {
    let raw = match mode {
        LayoutMode::ScrollVertical => stack::measure(
            layout,
            tree,
            node,
            Size::new(inner_avail.w, f32::INFINITY),
            Axis::Y,
            tc,
            out,
        ),
        LayoutMode::ScrollHorizontal => stack::measure(
            layout,
            tree,
            node,
            Size::new(f32::INFINITY, inner_avail.h),
            Axis::X,
            tc,
            out,
        ),
        LayoutMode::ScrollBoth => zstack::measure(layout, tree, node, Size::INF, tc, out),
        _ => unreachable!("scroll::measure called with non-Scroll mode {mode:?}"),
    };

    let wid = tree.records.widget_id()[node.idx()];
    // Roll up leading-edge origin from direct children (typically the
    // canvas published by `canvas::measure`). Non-canvas children
    // leave their slot at `(0,0)`, so the min collapses to `(0,0)` in
    // the common case.
    // Roll up published bbox.min from direct children. The cache
    // round-trips `content_origin` alongside `desired`, so a
    // measure-cache hit at any canvas descendant still surfaces the
    // right value here — no separate fallback path.
    let mut bb_min = Vec2::ZERO;
    for c in tree.active_children(node) {
        let co = layout.scratch.content_origin[c.idx()];
        bb_min.x = bb_min.x.min(co.x);
        bb_min.y = bb_min.y.min(co.y);
    }
    let entry = layout.scroll_states.entry(wid).or_default();
    // `content` = full bbox in scroll-content space; `min` is the
    // leading-edge offset rolled up above, `max = min + raw` is the
    // trailing-edge corner. Margin lives separately in
    // `content_margin` and is applied at clamp time only — bars
    // reflect the real content, not the padded slack region.
    entry.content = Rect {
        min: bb_min,
        size: raw,
    };

    match mode {
        LayoutMode::ScrollVertical => Size::new(raw.w, 0.0),
        LayoutMode::ScrollHorizontal => Size::new(0.0, raw.h),
        LayoutMode::ScrollBoth => Size::ZERO,
        _ => unreachable!(),
    }
}

/// Arrange dispatch arm for [`LayoutMode::Scroll`]. Delegates to
/// stack/zstack arrange so children land in `inner` (already deflated
/// by user padding), then writes the layout-derived fields onto the
/// state row: `viewport` is `inner.size`, overflow follows from
/// `content > viewport` per axis, `seen` flips to true after the
/// first arrange, and `offset` is re-clamped to the new bounds.
pub(crate) fn arrange(
    layout: &mut LayoutEngine,
    tree: &Tree,
    node: NodeId,
    inner: Rect,
    mode: LayoutMode,
    out: &mut Layout,
) {
    match mode {
        LayoutMode::ScrollVertical => stack::arrange(layout, tree, node, inner, Axis::Y, out),
        LayoutMode::ScrollHorizontal => stack::arrange(layout, tree, node, inner, Axis::X, out),
        LayoutMode::ScrollBoth => zstack::arrange(layout, tree, node, inner, out),
        _ => unreachable!("scroll::arrange called with non-Scroll mode {mode:?}"),
    }

    let wid = tree.records.widget_id()[node.idx()];
    // `outer` = the scroll widget's outer ZStack rect. `Scroll::show`
    // builds it as a wrapper that owns the bar-gutter reservation
    // padding, so its size is parent-allocated and stable across
    // reservation flips (unlike viewport, which shrinks when a gutter
    // appears). Used at record time to position bars flush with the
    // outer far edge. Falls back to `inner.size` for a root-mounted
    // scroll (no wrapper).
    let parent = tree.parents[node.idx()];
    let outer = if parent != NodeId::ROOT {
        out[layout.active_layer].rect[parent.idx()].size
    } else {
        inner.size
    };
    let entry = layout.scroll_states.entry(wid).or_default();
    let viewport = inner.size;
    let zoom = entry.zoom;
    entry.viewport = viewport;
    entry.outer = outer;
    entry.overflow = (
        entry.content.size.w * zoom > viewport.w,
        entry.content.size.h * zoom > viewport.h,
    );
    entry.seen = true;
    // No offset clamp here. Pivot-anchored zoom (in `Scroll::show`)
    // can legitimately drift `offset` outside `[0, slack]` to keep the
    // world point under the cursor fixed; clamping in arrange would
    // erase that drift every frame and break cursor anchoring during
    // continuous pinch through a content edge. The widget re-clamps
    // on actual pan input, which is the only place a stale offset
    // matters for the user.
}
