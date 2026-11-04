//! One node's five SoA columns, as `Node::cols` hands them over.

use crate::primitives::identity::widget_id::WidgetId;
use crate::scene::node::bounds_extras::BoundsExtras;
use crate::scene::node::layout_core::LayoutCore;
use crate::scene::node::node_flags::NodeFlags;
use crate::scene::node::panel_extras::PanelExtras;

/// The result of `Node::cols`, which `Tree::open_node` finishes where the chrome and the parent are
/// known: the clip it downgrades, the disabled bit a root inherits, the padding a border adds.
/// Refused: `Tree::open_node` reading a `Node` field by field, which spreads the fan-out across
/// the two types.
#[derive(Debug)]
pub(crate) struct NodeCols {
    pub(crate) widget_id: WidgetId,
    pub(crate) layout: LayoutCore,
    pub(crate) attrs: NodeFlags,
    pub(crate) bounds: BoundsExtras,
    pub(crate) panel: PanelExtras,
}
