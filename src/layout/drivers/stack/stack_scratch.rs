//! The stack driver's depth-shared pools.

use crate::layout::depth_scratch::DepthScratch;
use crate::layout::fill_item::FillItem;
use crate::layout::hug_item::HugItem;
use crate::scene::tree::node_id::NodeId;

/// The two pools one stack shares its main axis through: an entry per `Fill` child and one per other child, each depth working on its own tail. Kept together so a driver borrows both.
#[derive(Debug, Default)]
pub(crate) struct StackScratch {
    pub(super) fill: DepthScratch<FillItem<NodeId>>,
    pub(super) hug: DepthScratch<HugItem<NodeId>>,
}

#[cfg(test)]
pub(crate) mod internals {
    use crate::layout::drivers::stack::stack_scratch::StackScratch;

    impl StackScratch {
        /// Whether both pools drained, as a stack's exit leaves them.
        pub(crate) fn is_empty(&self) -> bool {
            self.fill.is_empty() && self.hug.is_empty()
        }
    }
}
