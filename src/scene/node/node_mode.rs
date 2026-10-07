//! A node's layout mode, and the two cases where it is not yet known (a grid or bar overlay built before its definition was interned).

use crate::primitives::layout::layout_mode::LayoutMode;
use std::mem;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum NodeMode {
    Resolved(LayoutMode),
    PendingGrid,
    PendingScrollbars,
}

impl NodeMode {
    /// Whether `mode` refines this one rather than replacing it.
    ///
    /// A grid's tracks and a bar overlay's definition arrive after the builder chain (it has no `Ui`) through [`Node::set_mode`](crate::scene::node::Node::set_mode); an installed mode refines a node, never turns a grid into a stack.
    #[inline]
    pub(super) fn accepts(self, mode: LayoutMode) -> bool {
        match self {
            Self::PendingGrid => matches!(mode, LayoutMode::Grid(_)),
            Self::PendingScrollbars => matches!(mode, LayoutMode::Scrollbars(_)),
            Self::Resolved(current) => mem::discriminant(&current) == mem::discriminant(&mode),
        }
    }

    #[inline(always)]
    pub(super) fn resolved(self) -> LayoutMode {
        match self {
            Self::Resolved(mode) => mode,
            Self::PendingGrid => {
                panic!("grid node recorded before its definition was installed")
            }
            Self::PendingScrollbars => {
                panic!("scrollbar overlay recorded before its definition was installed")
            }
        }
    }
}
