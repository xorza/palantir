//! One layer of the frame the damage diff reads.

use crate::cascade::layer_cascade::LayerCascade;
use crate::scene::layer::Layer;
use crate::scene::tree::Tree;

/// One layer's recorded tree and its cascade, as both per-layer diffs read them.
#[derive(Clone, Copy, Debug)]
pub(super) struct LayerFrame<'a> {
    pub(super) layer: Layer,
    pub(super) tree: &'a Tree,
    pub(super) cascade: &'a LayerCascade,
}
