pub(crate) mod button;
pub(crate) mod frame;
pub(crate) mod grid;
pub(crate) mod panel;
pub(crate) mod scroll;
pub(crate) mod text;
pub(crate) mod text_edit;
pub(crate) mod theme;

use crate::input::ResponseState;
use crate::layout::types::clip_mode::ClipMode;
use crate::primitives::corners::Corners;
use crate::primitives::rect::Rect;
use crate::tree::NodeId;
use crate::tree::element::Element;
use crate::widgets::theme::Background;

/// Resolve `ClipMode::Rounded` against the panel's effective background:
/// stamp `clip_radius` from `bg.radius` so the encoder doesn't have to
/// sniff the node's painted shapes. If the panel asked for rounded clip
/// but has no background or zero radius, silently downgrade to
/// `ClipMode::Rect` — a no-radius rounded clip is just a rectangular
/// clip, no surprise.
pub(crate) fn bind_clip_radius_to_background(element: &mut Element, bg: Option<&Background>) {
    if element.clip != ClipMode::Rounded {
        return;
    }
    match bg {
        Some(b) if b.radius != Corners::ZERO => element.clip_radius = Some(b.radius),
        _ => element.clip = ClipMode::Rect,
    }
}

#[cfg(test)]
mod tests;

pub struct Response {
    pub(crate) node: NodeId,
    pub state: ResponseState,
}

impl Response {
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
}
