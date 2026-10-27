//! The crate's `(plane, node)` address.

use crate::scene::plane::Plane;
use crate::scene::tree::node_id::NodeId;

/// Where one recorded node sits: its `NodeId` plus the plane whose tree holds it. A `NodeId` alone addresses nothing, as the index repeats across plane trees.
///
/// Read by layout, the cascade's `by_id` index, `Ui`'s hit-test consumers, and [`SeenIds`](crate::scene::seen_ids::SeenIds), which files both halves of an id collision as endpoints so the debug overlay can resolve each arranged rect without a tree scan.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Endpoint {
    pub(crate) plane: Plane,
    pub(crate) node: NodeId,
}
