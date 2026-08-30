//! The crate's `(layer, node)` address.

use crate::scene::layer::Layer;
use crate::scene::tree::node_id::NodeId;

/// Where one recorded node sits: its `NodeId` together with the layer
/// whose tree holds it. A `NodeId` alone addresses nothing — layers are
/// separate trees and the index repeats across them — so every column
/// keyed by position takes one of these.
///
/// Read by layout (`Layout::arranged_rect`, `scroll_content`), by the
/// cascade's `by_id` index, by `Ui`'s hit-test consumers, and by
/// [`SeenIds`](crate::scene::seen_ids::SeenIds), which files both halves
/// of an explicit-id collision as endpoints so the debug overlay can
/// resolve each side's arranged rect without a tree scan — even when the
/// pair straddles a `push_layer` boundary.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Endpoint {
    pub(crate) layer: Layer,
    pub(crate) node: NodeId,
}
