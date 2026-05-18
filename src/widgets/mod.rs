pub(crate) mod button;
pub(crate) mod checkbox;
pub(crate) mod context_menu;
pub(crate) mod frame;
pub(crate) mod grid;
pub(crate) mod panel;
pub(crate) mod popup;
pub(crate) mod radio;
pub(crate) mod scroll;
pub(crate) mod text;
pub(crate) mod text_edit;
pub(crate) mod theme;
pub(crate) mod tooltip;

use crate::input::ResponseState;
use crate::primitives::rect::Rect;
use crate::primitives::widget_id::WidgetId;
use glam::Vec2;

#[cfg(test)]
mod tests;

#[derive(Debug)]
pub struct Response {
    pub(crate) id: WidgetId,
    pub(crate) state: ResponseState,
}

impl Response {
    /// Widget id of the originating widget. Stable across frames (same
    /// hash) as long as the call-site / explicit-key inputs don't change.
    /// Pass to `ContextMenu::for_id` to attach state to this widget.
    pub fn widget_id(&self) -> WidgetId {
        self.id
    }
    pub fn rect(&self) -> Option<Rect> {
        self.state.rect
    }
    /// Pre-transform layout rect — see
    /// [`crate::input::ResponseState::layout_rect`].
    pub fn layout_rect(&self) -> Option<Rect> {
        self.state.layout_rect
    }
    pub fn hovered(&self) -> bool {
        self.state.hovered
    }
    pub fn pressed(&self) -> bool {
        self.state.pressed
    }
    pub fn clicked(&self) -> bool {
        self.state.clicked
    }
    /// One-frame edge: right mouse button clicked-and-released on this
    /// widget without latching a drag. Independent of `clicked` (left).
    pub fn secondary_clicked(&self) -> bool {
        self.state.secondary_clicked
    }
    /// Cumulative pointer travel since press while this widget holds
    /// the active, threshold-crossed drag. Compose against an anchor
    /// captured on `drag_started()`: `pos = anchor + delta`. `None`
    /// outside drag and for sub-threshold wiggle.
    pub fn drag_delta(&self) -> Option<Vec2> {
        self.state.drag_delta
    }
    /// One-frame edge: fires on exactly the frame the drag latches.
    /// Snapshot the position here to anchor subsequent `drag_delta`
    /// reads.
    pub fn drag_started(&self) -> bool {
        self.state.drag_started
    }
    /// Combined wheel + touchpad scroll delta this frame, in logical
    /// pixels. Routes only to widgets with [`crate::Sense::SCROLL`]
    /// that were the topmost scroll target under the pointer.
    /// `Vec2::ZERO` otherwise — and also when the widget *is* the
    /// target but no scroll event arrived this frame. Sign matches
    /// "advance offset forward" (positive = scroll down/right); see
    /// `widgets/scroll.rs` for the canonical `offset += delta`
    /// consumer.
    pub fn scroll_delta(&self) -> Vec2 {
        self.state.scroll_delta
    }
    /// Multiplicative pinch zoom factor this frame (`1.0` = no
    /// pinch). Routes to widgets with [`crate::Sense::PINCH`].
    /// Independent of `scroll_delta` so a list can pan-via-scroll
    /// without committing to pinch-to-zoom, and vice versa.
    pub fn zoom_factor(&self) -> f32 {
        self.state.zoom_factor
    }
    /// Cursor position relative to this widget's `rect.min`. `None`
    /// when the pointer is off-surface or the widget didn't arrange.
    /// Useful as a pivot for zoom-about-cursor without recomputing
    /// the rect origin from `rect()`.
    pub fn pointer_local(&self) -> Option<Vec2> {
        self.state.pointer_local
    }
}

/// `Response` plus a value returned by the body closure of widgets
/// that take one (`Panel`/`Grid`/`Scroll`). `Deref`s to `Response` so
/// callers ignoring the inner value keep working unchanged.
#[derive(Debug)]
pub struct InnerResponse<R> {
    pub response: Response,
    pub inner: R,
}

impl<R> std::ops::Deref for InnerResponse<R> {
    type Target = Response;
    fn deref(&self) -> &Response {
        &self.response
    }
}

#[cfg(any(test, feature = "internals"))]
pub mod test_support {
    #![allow(dead_code, private_interfaces)]
    use super::*;
    use crate::Ui;
    use crate::forest::tree::NodeId;

    impl Response {
        /// Old `Response.node` field as an inherent test-only method.
        pub fn node(&self, ui: &Ui) -> NodeId {
            ui.node_for_widget_id(self.id)
        }
    }
}
