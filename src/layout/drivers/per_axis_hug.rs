//! The measure ZStack and Canvas share: children that each sit at an offset and grow the panel per
//! axis.

use crate::layout::measured::ContentMeasure;
use crate::layout::pass::LayoutPass;
use crate::primitives::geometry::size::Size;
use crate::scene::tree::Tree;
use crate::scene::tree::node_id::NodeId;
use glam::Vec2;

/// Measure the children of a per-axis-hug panel (ZStack / Canvas) and return the
/// extent that covers them. `offset` is where the panel places a child (zero for
/// a ZStack, the declared position for a Canvas). Every axis offers the child the
/// room left past the offset (what `arrange` will hand it, or on a Hug axis the
/// most the panel can grow to), since offering more makes a wrapping child report
/// a height for a width it won't get. A bounded axis reports the child's own
/// extent; a Hug axis folds the offset back in.
pub(super) fn measure(
    pass: &mut LayoutPass<'_>,
    node: NodeId,
    inner_avail: Size,
    mut offset: impl FnMut(&Tree, NodeId) -> Vec2,
) -> ContentMeasure {
    let tree = pass.tree();
    let hug = tree.records.layout()[node.idx()].size.hug_mask();
    // The child holds while the room past its position stays past its range.
    let room_from = |from: f32, offset: f32| if from > 0.0 { from + offset } else { 0.0 };
    let mut max = ContentMeasure::ZERO;
    for c in tree.active_children(node) {
        let at = offset(tree, c);
        // A hug axis grows to cover where the child was put; a bounded one reports the
        // child's extent alone. The floor follows the extent.
        let child = pass.measure(c, inner_avail.room_past(at));
        let covered = |d: Size| Size::new(at.x + d.w, at.y + d.h).select(hug, d);
        max = ContentMeasure {
            size: max.size.max(covered(child.size)),
            floor: max.floor.max(covered(child.floor)),
            stable_from: max.stable_from.max(Size::new(
                room_from(child.stable_from.w, at.x),
                room_from(child.stable_from.h, at.y),
            )),
        };
    }
    max
}
