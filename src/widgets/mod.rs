mod button;
mod stack;
pub use button::Button;
pub use stack::{HStack, Stack, VStack};

use crate::tree::NodeId;

pub struct Response {
    pub node: NodeId,
}
