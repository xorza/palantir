//! One plane of the frame the damage diff reads.

use crate::cascade::plane_cascade::PlaneCascade;
use crate::scene::plane::Plane;
use crate::scene::tree::Tree;

/// One plane's recorded tree and its cascade, as both per-plane diffs read them.
#[derive(Clone, Copy, Debug)]
pub(super) struct PlaneFrame<'a> {
    pub(super) plane: Plane,
    pub(super) tree: &'a Tree,
    pub(super) cascade: &'a PlaneCascade,
}
