//! [`ResolvedScrollbarsDef`]: a bar overlay's definition as the tree stores it.

use crate::scene::scrollbars_def::ScrollbarsDef;
use crate::scene::tree::node_id::NodeId;

/// A [`ScrollbarsDef`] as the tree stores it, with the viewport resolved to the node; valid for the pass that resolved it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct ResolvedScrollbarsDef {
    pub(crate) def: ScrollbarsDef,
    pub(crate) content: NodeId,
}
