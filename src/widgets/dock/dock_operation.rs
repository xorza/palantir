//! The dock's one mutation vocabulary, and where a move lands a tab.

use serde::{Deserialize, Serialize};

use crate::widgets::dock::dock_path::DockPath;
use crate::widgets::dock::split_side::SplitSide;
use crate::widgets::dock::tab_group::TabGroupId;

/// Where a moved tab lands — the payload of [`DockOperation::MoveTab`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DockDrop {
    /// Join `group`'s strip at `index` (clamped to its length).
    Into {
        /// Target group.
        group: TabGroupId,
        /// Slot within that strip, clamped to its length.
        index: usize,
    },
    /// Split `group`'s pane; the tab becomes a fresh single-tab group on
    /// the given side.
    Split {
        /// Target group.
        group: TabGroupId,
        /// Which half of it the new group takes.
        side: SplitSide,
    },
}

/// One dock mutation, run by [`DockState::apply`](crate::DockState::apply) and transported by the application's queue or [`DockView::run`](crate::DockView::run). **Every operation tolerates a stale address**: it is built from the previous frame's response, so a vanished tab, group or split leaves the tree untouched. Tabs are named by identity, never strip position.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum DockOperation<T> {
    /// Make `tab` visible in its group and focus that group.
    ActivateTab {
        /// The tab.
        tab: T,
    },
    /// Open `tab` in the focused group (reusing it where it sits), make it visible and focus its pane.
    OpenTab {
        /// The tab.
        tab: T,
    },
    /// Close `tab` wherever it sits; the pinned tab refuses.
    CloseTab {
        /// The tab.
        tab: T,
    },
    /// Move `tab` to `to` — into another strip, or splitting a pane.
    MoveTab {
        /// The tab.
        tab: T,
        /// Destination.
        to: DockDrop,
    },
    /// Set the ratio of the split at `split` (its packed root path); emitted per drag frame, coalescing per split.
    SetRatio {
        /// The split.
        split: DockPath,
        /// Fraction the leading half takes, coerced into the split clamp; non-finite centres it.
        ratio: f32,
    },
    /// Move focus onto `group`, because a press landed inside its pane.
    FocusPane {
        /// The group.
        group: TabGroupId,
    },
}
