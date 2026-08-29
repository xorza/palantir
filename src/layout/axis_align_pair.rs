//! A child's resolved horizontal and vertical alignment.

use crate::layout::axis::Axis;
use crate::layout::types::align::{Align, AxisAlign};
use crate::scene::node::layout_core::LayoutCore;

/// Per-axis alignment after the child→parent `Auto` fallback — what
/// [`Self::resolve`] hands back.
#[derive(Clone, Copy, Debug)]
pub(super) struct AxisAlignPair {
    pub(super) h: AxisAlign,
    pub(super) v: AxisAlign,
}

impl AxisAlignPair {
    /// Neither axis aligned — what a caller passes when it positions the
    /// child by other means and wants only the arranged extents.
    pub(super) const AUTO: Self = Self {
        h: AxisAlign::Auto,
        v: AxisAlign::Auto,
    };

    /// Resolve a child's alignment on both axes: child's own value if not
    /// `Auto`, else the parent's `child_align` for that axis. Single source
    /// of truth for the alignment cascade — every layout (stack, grid,
    /// zstack) resolves through this or through [`Self::resolve_axis`], so
    /// they can't drift.
    pub(super) fn resolve(child: &LayoutCore, parent_child_align: Align) -> Self {
        Self {
            h: Self::resolve_axis(Axis::X, child, parent_child_align),
            v: Self::resolve_axis(Axis::Y, child, parent_child_align),
        }
    }

    /// One axis of the same cascade, for the drivers that place a child
    /// on one axis at a time — a stack reads only its cross axis, and
    /// resolving the pair there threw half of it away per child per
    /// frame.
    pub(super) fn resolve_axis(
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

    /// Both axes with `Auto` read as `Stretch` — Grid's default, where a
    /// child that named no alignment fills its cell.
    pub(super) const fn or_stretch_if_auto(self) -> Self {
        Self {
            h: self.h.or_stretch_if_auto(),
            v: self.v.or_stretch_if_auto(),
        }
    }
}
