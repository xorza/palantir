//! The surface a tree test records against, and the hashes it asserts on.

use crate::Ui;
use crate::common::content_hash::ContentHash;
use crate::internals::harness::UiHarness;
use crate::scene::layer::Layer;
use crate::scene::tree::node_id::NodeId;
use glam::UVec2;

pub(super) const SURFACE: UVec2 = UVec2::new(200, 200);

/// The hashes of one recorded frame: the returned node, its subtree and layout half, and the tree's cascade-static fingerprint.
#[derive(Clone, Copy, Debug)]
pub(super) struct Hashes {
    pub(super) node: ContentHash,
    pub(super) subtree: ContentHash,
    pub(super) layout_subtree: ContentHash,
    pub(super) cascade_static: ContentHash,
}

/// Record one frame of `f` on a fresh harness and read the hashes of the
/// node it returns.
pub(super) fn record(mut f: impl FnMut(&mut Ui) -> NodeId) -> Hashes {
    let mut h = UiHarness::new(SURFACE);
    let target = h.frame_value(|ui| f(ui));
    let tree = h.ui.tree(Layer::Main);
    Hashes {
        node: tree.rollups.node[target.idx()],
        subtree: tree.rollups.subtree[target.idx()],
        layout_subtree: tree.rollups.layout_subtree[target.idx()],
        cascade_static: tree.fingerprint.cascade_static,
    }
}
