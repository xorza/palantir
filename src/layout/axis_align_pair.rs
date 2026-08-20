//! A child's resolved horizontal and vertical alignment.

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
    /// zstack) calls this so they can't drift. Stack discards the unused
    /// axis; the cost is two enum matches per child per frame.
    pub(super) fn resolve(child: &LayoutCore, parent_child_align: Align) -> Self {
        let a = child.meta.align();
        Self {
            h: a.halign().or(parent_child_align.halign()).to_axis(),
            v: a.valign().or(parent_child_align.valign()).to_axis(),
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
