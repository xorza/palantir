//! The surface a tree test records against, and the three hash reads it
//! asserts on.

use crate::Ui;
use crate::common::content_hash::ContentHash;
use crate::scene::layer::Layer;
use crate::scene::tree::node_id::NodeId;
use crate::ui::harness::UiHarness;
use glam::UVec2;

pub(super) const SURFACE: UVec2 = UVec2::new(200, 200);

pub(super) fn record_hash<F: FnMut(&mut Ui) -> NodeId>(mut f: F) -> ContentHash {
    let mut h = UiHarness::new(SURFACE);
    let target = h.frame_value(|ui| f(ui));
    h.ui.tree(Layer::Main).rollups.node[target.idx()]
}

pub(super) fn record_cascade_static<F: FnMut(&mut Ui) -> NodeId>(mut f: F) -> ContentHash {
    let mut h = UiHarness::new(SURFACE);
    let _ = h.frame_value(|ui| f(ui));
    h.ui.tree(Layer::Main).fingerprint.cascade_static
}
