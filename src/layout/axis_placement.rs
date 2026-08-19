//! One axis's arranged extent and its alignment offset.

use crate::layout::axis::Axis;
use crate::layout::axis_align_pair::AxisAlignPair;
use crate::layout::types::align::{Align, AxisAlign};
use crate::primitives::size::Size;
use crate::scene::node::bounds_extras::BoundsExtras;
use crate::scene::node::layout_core::LayoutCore;

/// Per-axis placement: chosen extent + offset within the parent's inner span.
#[derive(Debug)]
pub(super) struct AxisPlacement {
    pub(super) size: f32,
    pub(super) offset: f32,
}

impl AxisPlacement {
    /// Resolve the outer extent and alignment offset for one arranged axis.
    /// `Fixed` always keeps its measured extent. `Fill` and explicit `Stretch`
    /// grow to their slot without shrinking below measured content, while the
    /// node's outer min/max bounds remain authoritative.
    pub(super) fn arrange(
        axis: Axis,
        align: AxisAlign,
        child: &LayoutCore,
        bounds: &BoundsExtras,
        desired: Size,
        slot: f32,
    ) -> Self {
        let margin = axis.spacing(child.margin);
        let min = axis.main(bounds.min_size) + margin;
        let max = axis.main(bounds.max_size) + margin;
        let desired = axis.main(desired).clamp(min, max);
        let sizing = axis.main_sizing(child.size);
        let stretch = sizing.fill_weight().is_some()
            || matches!(align, AxisAlign::Stretch) && sizing.fixed_value().is_none();
        let size = if stretch {
            slot.max(desired).clamp(min, max)
        } else {
            desired
        };
        let offset = match align {
            AxisAlign::Center => ((slot - size) * 0.5).max(0.0),
            AxisAlign::End => (slot - size).max(0.0),
            _ => 0.0,
        };
        Self { size, offset }
    }

    /// Outer size of a node arranged into `slot` on both axes with no
    /// alignment — [`Self::arrange`] twice under [`AxisAlign::Auto`], keeping
    /// only the extents.
    ///
    /// The two callers that place a node without needing its alignment offset:
    /// `LayoutEngine::run` sizing a layer root against the surface, and
    /// `canvas::arrange` sizing an absolutely-positioned child against its
    /// slot. Both position by other means (the root's `Placement`, the child's
    /// declared `pos`), so the offset `Self::arrange` returns is dead to them.
    /// Grid and ZStack call `Self::arrange` directly precisely because they do
    /// use that offset.
    pub(super) fn arrange_size(
        child: &LayoutCore,
        bounds: &BoundsExtras,
        desired: Size,
        slot: Size,
    ) -> Size {
        Size::new(
            Self::arrange(Axis::X, AxisAlign::Auto, child, bounds, desired, slot.w).size,
            Self::arrange(Axis::Y, AxisAlign::Auto, child, bounds, desired, slot.h).size,
        )
    }

    /// Cross-axis placement for a child of a main-axis stack (Stack /
    /// WrapStack). Resolves the alignment cascade, picks the cross axis
    /// from the resolved (h, v) pair, and runs [`Self::arrange`] against the
    /// child's cross sizing + desired + the parent's cross extent. Single
    /// source of truth so the cascade rule can't drift between Stack and
    /// WrapStack.
    pub(super) fn cross(
        main_axis: Axis,
        child: &LayoutCore,
        bounds: &BoundsExtras,
        parent_child_align: Align,
        desired: Size,
        inner_cross: f32,
    ) -> Self {
        let AxisAlignPair { h, v } = AxisAlignPair::resolve(child, parent_child_align);
        let cross_align = match main_axis {
            Axis::X => v,
            Axis::Y => h,
        };
        let cross_axis = match main_axis {
            Axis::X => Axis::Y,
            Axis::Y => Axis::X,
        };
        Self::arrange(cross_axis, cross_align, child, bounds, desired, inner_cross)
    }
}
