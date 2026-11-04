//! [`Inherited`]: what a node hands its descendants of disabled and visible.

use crate::primitives::layout::visibility::Visibility;
use crate::scene::node::node_flags::NodeFlags;

/// Whether a node is disabled and whether it is invisible, each held when the node or an ancestor
/// holds it. The record pass reads it a frame ahead of the cascade, to gate panes and paint
/// animations, and the cascade to gate paint and input; both derive it through [`Self::child`].
/// Refused: the rule written in each pass, which only a debug paint-row assert held equal.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Inherited {
    pub(crate) disabled: bool,
    pub(crate) invisible: bool,
}

impl Inherited {
    /// What a root inherits: nothing. A root raised from a disabled scope carries its own
    /// disabled bit.
    pub(crate) const ROOT: Self = Self {
        disabled: false,
        invisible: false,
    };

    /// What a child of this node holds, given its own `attrs` and `visibility`.
    #[inline]
    pub(crate) const fn child(self, attrs: NodeFlags, visibility: Visibility) -> Self {
        Self {
            disabled: self.disabled || attrs.is_disabled(),
            invisible: self.invisible || !visibility.is_visible(),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::primitives::layout::visibility::Visibility;
    use crate::scene::node::inherited::Inherited;
    use crate::scene::node::node_flags::NodeFlags;

    /// Each bit holds where the node or an ancestor holds it: a disabled or a hidden or collapsed
    /// node sets its bit, a parent's bit passes to every child, and an enabled visible child of a
    /// root holds neither.
    #[test]
    fn a_node_inherits_what_it_or_an_ancestor_holds() {
        let mut disabled = NodeFlags::default();
        disabled.set_disabled(true);
        let enabled = NodeFlags::default();
        let both = Inherited {
            disabled: true,
            invisible: true,
        };
        assert_eq!(
            Inherited::ROOT.child(enabled, Visibility::Visible),
            Inherited::ROOT
        );
        assert_eq!(
            Inherited::ROOT.child(disabled, Visibility::Visible),
            Inherited {
                disabled: true,
                invisible: false,
            }
        );
        for hidden in [Visibility::Hidden, Visibility::Collapsed] {
            assert_eq!(
                Inherited::ROOT.child(enabled, hidden),
                Inherited {
                    disabled: false,
                    invisible: true,
                }
            );
        }
        assert_eq!(both.child(enabled, Visibility::Visible), both);
    }
}
