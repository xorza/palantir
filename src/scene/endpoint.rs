//! The crate's `(layer, node)` address.

use crate::scene::layer::Layer;
use crate::scene::tree::node_id::NodeId;

/// Where one recorded node sits: its `NodeId` plus the layer whose tree holds it. A `NodeId` alone addresses nothing, as the index repeats across layer trees.
///
/// Read by layout, the cascade's `by_id` index, `Ui`'s hit-test consumers, and [`SeenIds`](crate::scene::seen_ids::SeenIds), which files both halves of an id collision as endpoints so the debug overlay can resolve each arranged rect without a tree scan.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Endpoint {
    pub(crate) layer: Layer,
    pub(crate) node: NodeId,
}
