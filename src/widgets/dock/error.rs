//! What [`DockState::validate`](crate::DockState::validate) rejects: a
//! tree that broke one of the invariants the module doc lists.

use crate::widgets::dock::tab_group::TabGroupId;

/// A structural violation found in a deserialized dock tree.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DockError<T> {
    /// The nodes are not in canonical pre-order.
    NonCanonical,
    /// A child index points past the end of the node vector.
    NodeOutOfRange { index: u32 },
    /// A root-to-leaf chain nests deeper than the cap allows.
    SplitNesting,
    /// A split ratio sits outside the clamp.
    SplitRatio { ratio: f32 },
    /// The vector holds slots the root cannot reach.
    UnreachableSlots,
    /// No group holds the pinned tab.
    MissingPinnedTab,
    /// Two groups share an id, so every op addressed to it is ambiguous.
    DuplicateGroup { group: TabGroupId },
    /// A group holds no tabs.
    EmptyGroup { group: TabGroupId },
    /// A group's visible index points past its own tab list.
    ActiveTabOutOfRange { group: TabGroupId },
    /// One tab appears in two places.
    DuplicateTab { tab: T },
    /// The focused group is not in the tree.
    MissingFocusedGroup { group: TabGroupId },
}

impl<T: std::fmt::Debug> std::fmt::Display for DockError<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NonCanonical => write!(f, "dock nodes are not in canonical pre-order"),
            Self::NodeOutOfRange { index } => write!(f, "dock node index {index} out of range"),
            Self::SplitNesting => write!(f, "split nesting exceeds the cap"),
            Self::SplitRatio { ratio } => write!(f, "split ratio {ratio} out of bounds"),
            Self::UnreachableSlots => write!(f, "dock tree has slots unreachable from the root"),
            Self::MissingPinnedTab => write!(f, "no group holds the pinned tab"),
            Self::DuplicateGroup { group } => write!(f, "dock group {group:?} appears twice"),
            Self::EmptyGroup { group } => write!(f, "dock group {group:?} is empty"),
            Self::ActiveTabOutOfRange { group } => {
                write!(f, "dock group {group:?} active tab out of range")
            }
            Self::DuplicateTab { tab } => write!(f, "tab {tab:?} appears twice"),
            Self::MissingFocusedGroup { group } => write!(f, "focused group {group:?} is missing"),
        }
    }
}

impl<T: std::fmt::Debug> std::error::Error for DockError<T> {}
