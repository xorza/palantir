//! The dock's one mutation vocabulary, and where a move lands a tab.

use serde::{Deserialize, Serialize};

use crate::widgets::dock::dock_path::DockPath;
use crate::widgets::dock::split_side::SplitSide;
use crate::widgets::dock::tab_group::TabGroupId;
use crate::widgets::splitter::split_size::SplitSize;

/// Where a moved tab lands — the payload of [`DockOperation::MoveTab`].
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum DockDrop {
    /// Join `group`'s strip at `idx` (clamped to its length).
    Join {
        /// Target group.
        group: TabGroupId,
        /// Slot within that strip, clamped to its length.
        idx: usize,
    },
    /// Split `group`'s pane; the tab becomes a fresh single-tab group on
    /// the given side, the split shared as `size`. A split is sized where it is made, as
    /// egui_dock's `split_right` takes its fraction
    /// (<https://docs.rs/egui_dock/latest/egui_dock/>), so no caller has to address a split it
    /// cannot name; a tab dragged onto a pane's edge splits at [`SplitSize::HALF`].
    Split {
        /// Target group.
        group: TabGroupId,
        /// Which half of it the new group takes.
        side: SplitSide,
        /// How the split shares the pane: a ratio, coerced into the split clamp, or one half's
        /// pixels.
        size: SplitSize,
    },
}

/// One dock mutation, run by [`DockState::apply`](crate::DockState::apply) and transported by the application's queue or an applying [`DockView`](crate::DockView). **Every operation tolerates a stale address**: it is built from the previous frame's response, so a vanished tab, group or split leaves the tree untouched. Tabs are named by identity, never strip position.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
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
    /// Set the size of the split at `split` (its packed root path); emitted per drag frame, coalescing per split.
    SetSize {
        /// The split.
        split: DockPath,
        /// A ratio, coerced into the split clamp and centered when not finite, or one half's
        /// pixels, ignored unless a finite length.
        size: SplitSize,
    },
    /// Move focus onto `group`, because a press landed inside its pane.
    FocusGroup {
        /// The group.
        group: TabGroupId,
    },
}
