//! Layout-side scroll subsystem. Owns:
//!
//! - **Driver** ([`measure`] + [`arrange`]) — minimal: INF-axis
//!   measure records the content extent into the matching
//!   [`ScrollLayoutState`] row, and standard arrange refreshes the
//!   layout-derived fields (viewport, overflow, seen). No separate
//!   post-pass — the driver is the refresh.
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

use crate::forest::tree::Tree;
use crate::forest::tree::node::NodeId;
use crate::layout::Layout;
use crate::layout::axis::Axis;
use crate::primitives::rect::Rect;
use crate::primitives::size::Size;
use crate::primitives::spacing::Spacing;
use crate::primitives::widget_id::WidgetId;
use glam::Vec2;
use rustc_hash::FxHashMap;

use crate::layout::engine::LayoutEngine;
use crate::layout::stack;
use crate::layout::support::TextCtx;
use crate::layout::types::layout_mode::ScrollSpec;
use crate::layout::zstack;

/// Cross-frame state row for one scroll widget. Owned by
/// [`LayoutEngine::scroll_states`] — *not* `StateMap`, because this
/// is a layout-derived concern (the driver writes layout fields) and
/// belongs in the layout subsystem rather than tangled with widget
/// state.
///
/// The widget at record time:
/// - Reads the snapshot via [`Ui::scroll_state`](crate::Ui::scroll_state)
///   for offset clamp + bar thumb geometry.
/// - Mutates `offset` from input (via the same entry).
///
/// The driver writes layout-derived fields:
/// - `measure` records `content` (the panned-axis extent).
/// - `arrange` records `viewport` (inner rect post user-padding +
///   constant bar-gutter reservation), `overflow`, and `seen`.
///
/// - `offset` — input-accumulated pan position (next frame's start).
/// - `viewport` — INNER (user-padding-deflated) size: what children
///   see. Drives `content > viewport` overflow checks.
/// - `outer` — full arranged rect size of the scroll node including
///   the bar gutter. Drives bar positioning so the bar sits flush
///   with the OUTER far edge. Parent-allocated and stable across
///   frames (reservation is constant on the pan axes, not toggled by
///   overflow).
/// - `content` — measured content extent on the panned axes.
/// - `overflow` — `(x, y)` per-axis: did this axis's content overflow
///   the viewport on the most recent measure? Read at record time to
///   decide whether to **draw** the bar thumb (the gutter is reserved
///   either way).
/// - `seen` — set true by `arrange` after the first frame. Read by
///   the widget to detect a cold-mount and trigger a relayout pass
///   so pass B records with correct overflow-driven thumb visibility.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ScrollLayoutState {
    pub(crate) offset: Vec2,
    /// Uniform zoom; `1.0` = no zoom. Mutated only by [`Scroll`] widgets
    /// configured via `with_zoom*`. The driver leaves it alone.
    pub(crate) zoom: f32,
    pub(crate) viewport: Size,
    pub(crate) outer: Size,
    /// Unscaled content extent. Multiply by `zoom` for the
    /// user-perceived (post-paint) extent. Margin lives separately
    /// on `content_margin`.
    pub(crate) content: Size,
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
            content: Size::ZERO,
            overflow: (false, false),
            seen: false,
            drag_anchor: None,
            content_margin: Spacing::default(),
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct OffsetEndpoints {
    leading: Vec2,
    trailing: Vec2,
}

/// Ordered offset bounds, per axis.
#[derive(Clone, Copy, Debug)]
struct OffsetBounds {
    lo: Vec2,
    hi: Vec2,
}

/// Theme-derived click-to-page geometry for one axis, computed by the
/// scroll widget (from the bar geometry) and handed to
/// [`ScrollLayoutState::apply_track_page`]. Primitives only — the layout
/// state stays unaware of `ScrollbarTheme`.
pub(crate) struct TrackPage {
    pub(crate) click_main: f32,
    pub(crate) thumb_offset: f32,
    pub(crate) thumb_size: f32,
    pub(crate) page_step: f32,
    pub(crate) max_off: f32,
}

/// Input-driven `offset` / `zoom` mutation. The scroll widget computes
/// the per-frame inputs (wheel delta, pivot, thumb-drag state, bar
/// geometry) and calls these; the offset-write invariant lives here on
/// the type that owns `offset`, not in `Scroll::show`. All inputs are
/// primitives so layout stays decoupled from widgets / input / theme.
impl ScrollLayoutState {
    fn offset_endpoints(&self) -> OffsetEndpoints {
        let [cml, cmt, cmr, cmb] = self.content_margin.as_array();
        OffsetEndpoints {
            leading: Vec2::new(-cml * self.zoom, -cmt * self.zoom),
            trailing: Vec2::new(
                self.content.w * self.zoom - self.viewport.w + cmr * self.zoom,
                self.content.h * self.zoom - self.viewport.h + cmb * self.zoom,
            ),
        }
    }

    /// Settled bounds collapse an underflowing axis at its leading edge;
    /// ordinary scrolls cannot pan into empty viewport space.
    fn natural_bounds(&self) -> OffsetBounds {
        let endpoints = self.offset_endpoints();
        OffsetBounds {
            lo: endpoints.leading,
            hi: endpoints.trailing.max(endpoints.leading),
        }
    }

    /// Pivot zoom can place undersized content between the raw leading
    /// and trailing endpoints, so zoomable pan preserves that ordered
    /// interval as its rubber-band range.
    fn zoom_rubber_band_bounds(&self) -> OffsetBounds {
        let endpoints = self.offset_endpoints();
        OffsetBounds {
            lo: endpoints.leading.min(endpoints.trailing),
            hi: endpoints.leading.max(endpoints.trailing),
        }
    }

    /// Pivot-anchored zoom step: clamp `zoom · delta` to
    /// `[min_zoom, max_zoom]`, then shift `offset` so the widget-local
    /// `pivot` point stays fixed across the scale change. No-op when the
    /// effective scale is ~1.
    pub(crate) fn apply_zoom(
        &mut self,
        min_zoom: f32,
        max_zoom: f32,
        pivot: Vec2,
        zoom_delta: f32,
    ) {
        let new_zoom = (self.zoom * zoom_delta).clamp(min_zoom, max_zoom);
        let dz_eff = if self.zoom > 0.0 {
            new_zoom / self.zoom
        } else {
            1.0
        };
        if (dz_eff - 1.0).abs() > f32::EPSILON {
            self.offset = (self.offset + pivot) * dz_eff - pivot;
            self.zoom = new_zoom;
        }
    }

    /// Wheel/touchpad pan. Zoomable scrolls retain the ordered underflow
    /// interval used by pivot zoom; settled scrolls use semantic natural
    /// bounds. Each range is extended to include the current offset, so
    /// a pan toward it works while a pan further out is blocked. Only
    /// nonzero deltas clamp (a pure-zoom frame leaves `offset` alone).
    pub(crate) fn apply_wheel_pan(
        &mut self,
        pan_x: bool,
        pan_y: bool,
        pan_delta: Vec2,
        preserve_zoom_underflow: bool,
    ) {
        let b = if preserve_zoom_underflow {
            self.zoom_rubber_band_bounds()
        } else {
            self.natural_bounds()
        };
        if pan_x && pan_delta.x != 0.0 {
            let lo = self.offset.x.min(b.lo.x);
            let hi = self.offset.x.max(b.hi.x);
            self.offset.x = (self.offset.x + pan_delta.x).clamp(lo, hi);
        }
        if pan_y && pan_delta.y != 0.0 {
            let lo = self.offset.y.min(b.lo.y);
            let hi = self.offset.y.max(b.hi.y);
            self.offset.y = (self.offset.y + pan_delta.y).clamp(lo, hi);
        }
    }

    /// Clamp `offset` straight to settled natural bounds on both axes.
    /// Used by non-zoomable scrolls each frame, where underflow collapses
    /// at the leading edge and shrunk content cannot strand a stale
    /// offset past the now-empty tail. Zoomable scrolls skip this because
    /// pivot anchoring legitimately leaves their offset out of range.
    pub(crate) fn clamp_to_natural(&mut self) {
        let b = self.natural_bounds();
        self.offset.x = self.offset.x.clamp(b.lo.x, b.hi.x);
        self.offset.y = self.offset.y.clamp(b.lo.y, b.hi.y);
    }

    /// Thumb-drag pan on `axis`. Snapshots `offset` into `drag_anchor`
    /// on the `drag_started` edge, then composes `offset.main =
    /// anchor.main + drag_delta.main · factor` (cumulative delta against
    /// a stable anchor keeps it idempotent across re-records). `geom` is
    /// the theme-derived `(factor, max_off)` for this axis, `None` when
    /// the bar has no thumb.
    pub(crate) fn apply_thumb_drag(
        &mut self,
        axis: Axis,
        drag_started: bool,
        drag_delta: Option<Vec2>,
        geom: Option<(f32, f32)>,
    ) {
        if drag_started {
            self.drag_anchor = Some((axis, self.offset));
        }
        let Some((anchor_axis, anchor)) = self.drag_anchor else {
            return;
        };
        if anchor_axis != axis {
            return;
        }
        let Some(delta) = drag_delta else {
            // Drag ended on this thumb — drop the anchor so the next
            // press starts a fresh snapshot.
            self.drag_anchor = None;
            return;
        };
        let Some((factor, max_off)) = geom else {
            return;
        };
        let target = axis.main_v(anchor) + axis.main_v(delta) * factor;
        let clamped = target.clamp(0.0, max_off);
        match axis {
            Axis::X => self.offset.x = clamped,
            Axis::Y => self.offset.y = clamped,
        }
    }

    /// Click-on-track paging on `axis`: a press above the thumb pages
    /// `offset` back one viewport, below it pages forward, clamped to
    /// `[0, max_off]` (toward-natural only). `page` is `None` when there
    /// was no qualifying click this frame.
    pub(crate) fn apply_track_page(&mut self, axis: Axis, page: Option<TrackPage>) {
        let Some(p) = page else {
            return;
        };
        let cur = axis.main_v(self.offset);
        let next = if p.click_main < p.thumb_offset {
            (cur - p.page_step).max(0.0)
        } else if p.click_main > p.thumb_offset + p.thumb_size {
            (cur + p.page_step).min(p.max_off)
        } else {
            cur
        };
        match axis {
            Axis::X => self.offset.x = next,
            Axis::Y => self.offset.y = next,
        }
    }
}

/// Cross-frame map of [`ScrollLayoutState`] keyed by the inner
/// viewport's `WidgetId`. Lives on [`LayoutEngine`]; the driver
/// writes layout-derived fields, the widget mutates `offset` from
/// input.
pub(crate) type ScrollStates = FxHashMap<WidgetId, ScrollLayoutState>;

/// Measure dispatch arm for `LayoutMode::Scroll`. Single
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
    spec: ScrollSpec,
    tc: &TextCtx<'_>,
    out: &mut Layout,
) -> Size {
    let pan = spec.pan_mask();
    // A panned axis the user sized `Hug` "fits" to content (reports its
    // content extent below); `Fill`/`Fixed` keep the content-independent
    // viewport (reports zero). The `Scroll` widget encodes the user's
    // per-axis `Sizing` into these bits, so `Hug` means the same "size to
    // content" here as it does for every other widget.
    let fit = spec.fit_mask();
    let child_avail = Size::new(
        if pan.x { f32::INFINITY } else { inner_avail.w },
        if pan.y { f32::INFINITY } else { inner_avail.h },
    );
    let raw = if pan.x && pan.y {
        zstack::measure(layout, tree, node, child_avail, tc, out)
    } else if pan.y {
        stack::measure(layout, tree, node, child_avail, Axis::Y, tc, out)
    } else {
        stack::measure(layout, tree, node, child_avail, Axis::X, tc, out)
    };

    let wid = tree.records.widget_id()[node.idx()];
    // Margin doesn't fold into `content` — it's applied at clamp
    // time only, so bars reflect the real bbox and the margin acts
    // as invisible overscroll. Negative-origin canvases are handled
    // entirely in userspace via
    // [`crate::widgets::scroll::Scroll::anchor_canvas_origin`]; the
    // driver itself sees only non-negative content.
    layout.scroll_states.entry(wid).or_default().content = raw;

    // A `Fill`/`Fixed` panned axis contributes zero to the viewport's own
    // desired size (content extent doesn't grow the viewport); non-panned
    // axes pass the measured size through. A `Hug` panned axis instead
    // reports the content extent, so the viewport sizes to content (and a
    // `max_size`/`available` cap then bounds it, with the overflow
    // scrolling). The content intrinsic stays zero (see `intrinsic.rs`),
    // so ancestors can still shrink a `Hug` viewport below its content.
    Size::new(
        if pan.x && !fit.x { 0.0 } else { raw.w },
        if pan.y && !fit.y { 0.0 } else { raw.h },
    )
}

/// Arrange dispatch arm for `LayoutMode::Scroll`. Delegates to
/// stack/zstack arrange so children land in `inner` (already deflated
/// by user padding), then writes the layout-derived fields onto the
/// state row: `viewport` is `inner.size`, overflow follows from
/// `content > viewport` per axis, and `seen` flips to true after the
/// first arrange.
pub(crate) fn arrange(
    layout: &mut LayoutEngine,
    tree: &Tree,
    node: NodeId,
    parent_outer: Size,
    inner: Rect,
    spec: ScrollSpec,
    out: &mut Layout,
) {
    let pan = spec.pan_mask();
    if pan.x && pan.y {
        zstack::arrange(layout, tree, node, inner, out);
    } else if pan.y {
        stack::arrange(layout, tree, node, inner, Axis::Y, out);
    } else {
        stack::arrange(layout, tree, node, inner, Axis::X, out);
    }

    let wid = tree.records.widget_id()[node.idx()];
    let entry = layout.scroll_states.entry(wid).or_default();
    let viewport = inner.size;
    let zoom = entry.zoom;
    entry.viewport = viewport;
    // `outer` = the scroll widget's outer ZStack rect (the wrapper
    // `Scroll::show` builds around the inner viewport). The inner
    // carries the constant bar-gutter reservation in its margin, so
    // `viewport = outer - margin - user_padding`. Used at record time
    // to position bars flush with the outer far edge. Engine passes
    // the parent's outer size directly; for a root-mounted scroll
    // (no wrapper), the engine forwards the root's own slot size,
    // which is a sensible fallback.
    entry.outer = parent_outer;
    entry.overflow = (
        entry.content.w * zoom > viewport.w,
        entry.content.h * zoom > viewport.h,
    );
    entry.seen = true;
    // No offset clamp here. Pivot-anchored zoom (in `Scroll::show`)
    // can legitimately drift `offset` outside `[0, slack]` to keep the
    // world point under the cursor fixed; clamping in arrange would
    // erase that drift every frame and break cursor anchoring during
    // continuous pinch through a content edge. The widget re-clamps
    // on actual pan input; non-zoomable scrolls also settle their offset
    // at record time.
}

#[cfg(test)]
mod tests;
