mod button;
mod frame;
mod panel;
mod stack;

pub use button::{Button, ButtonStyle};
pub use frame::Frame;
pub use panel::Panel;
pub use stack::{HStack, Stack, VStack};

use crate::input::ResponseState;
use crate::primitives::Rect;
use crate::tree::NodeId;

#[cfg(test)]
mod tests;

pub struct Response {
    pub node: NodeId,
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
