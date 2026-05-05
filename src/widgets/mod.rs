pub(crate) mod button;
pub(crate) mod frame;
pub(crate) mod grid;
pub(crate) mod panel;
pub(crate) mod scroll;
pub(crate) mod styled;
pub(crate) mod text;
pub(crate) mod theme;

use crate::input::ResponseState;
use crate::primitives::rect::Rect;
use crate::tree::NodeId;

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
