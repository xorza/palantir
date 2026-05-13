pub(crate) mod button;
pub(crate) mod context_menu;
pub(crate) mod frame;
pub(crate) mod grid;
pub(crate) mod panel;
pub(crate) mod popup;
pub(crate) mod scroll;
pub(crate) mod text;
pub(crate) mod text_edit;
pub(crate) mod theme;
pub(crate) mod tooltip;

use crate::forest::tree::NodeId;
use crate::forest::widget_id::WidgetId;
use crate::input::ResponseState;
use crate::primitives::rect::Rect;
use glam::Vec2;

#[cfg(test)]
mod tests;

pub struct Response {
    #[allow(dead_code)] // Read only from `#[cfg(test)]` modules.
    pub(crate) node: NodeId,
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
}
