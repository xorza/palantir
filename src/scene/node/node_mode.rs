//! A node's layout mode, and the two cases where it is not known yet — a
//! grid or a bar overlay built before its definition was interned.

use crate::layout::types::layout_mode::LayoutMode;
use std::mem;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum NodeMode {
    Resolved(LayoutMode),
    PendingGrid,
    PendingScrollbars,
}

impl NodeMode {
    /// Whether `mode` refines this one rather than replacing it with
    /// something else.
    ///
    /// Two nodes learn their payload after the builder chain has run,
    /// because it lives in the tree and the chain has no `Ui`: a grid
    /// takes its tracks, and a bar overlay its definition. Both arrive
    /// through [`Node::set_mode`](crate::scene::node::Node::set_mode), and
    /// this is the rule it holds them to — an installed mode refines a
    /// node, it never turns a grid into a stack.
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
