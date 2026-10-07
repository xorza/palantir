//! A child's resolved horizontal and vertical alignment.

use crate::primitives::layout::align::{Align, AxisAlign};
use crate::primitives::layout::axis::Axis;
use crate::scene::node::layout_core::LayoutCore;

/// Per-axis alignment after the child→parent `Auto` fallback.
#[derive(Clone, Copy, Debug)]
pub(super) struct AxisAlignPair {
    pub(super) h: AxisAlign,
    pub(super) v: AxisAlign,
}

impl AxisAlignPair {
    /// Neither axis aligned, for a caller that positions the child itself.
    pub(super) const AUTO: Self = Self {
        h: AxisAlign::Auto,
        v: AxisAlign::Auto,
    };

    /// Resolve both axes: the child's own value unless `Auto`, else the parent's `child_align`. Every layout resolves through this or [`Self::resolve_axis`].
    pub(super) const fn resolve(child: &LayoutCore, parent_child_align: Align) -> Self {
        Self {
            h: Self::resolve_axis(Axis::X, child, parent_child_align),
            v: Self::resolve_axis(Axis::Y, child, parent_child_align),
        }
    }

    /// One axis of the same cascade, for drivers placing on one axis (a stack reads only its cross axis).
    pub(super) const fn resolve_axis(
        axis: Axis,
        child: &LayoutCore,
        parent_child_align: Align,
    ) -> AxisAlign {
        let a = child.meta.align();
        match axis {
            Axis::X => a.halign().or(parent_child_align.halign()).to_axis(),
            Axis::Y => a.valign().or(parent_child_align.valign()).to_axis(),
        }
    }

    /// Both axes with `Auto` read as `Stretch`, Grid's default.
    pub(super) const fn or_stretch_if_auto(self) -> Self {
        Self {
            h: self.h.or_stretch_if_auto(),
            v: self.v.or_stretch_if_auto(),
        }
    }
}
