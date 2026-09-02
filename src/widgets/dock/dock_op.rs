//! The one mutation vocabulary the dock speaks, and where its move op
//! lands a tab.

use serde::{Deserialize, Serialize};

use crate::widgets::dock::dock_path::DockPath;
use crate::widgets::dock::split_side::SplitSide;
use crate::widgets::dock::tab_group::TabGroupId;

/// Where a moved tab lands — the payload of [`DockOp::MoveTab`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DockDrop {
    /// Join `group`'s strip at `index` (clamped to its length).
    Into { group: TabGroupId, index: usize },
    /// Split `group`'s pane; the tab becomes a fresh single-tab group on
    /// the given side.
    Split { group: TabGroupId, side: SplitSide },
}

/// One dock mutation, executed by
/// [`DockState::apply`](crate::DockState::apply).
///
/// The single vocabulary the whole pipeline speaks: the widget (or a
/// menu item, or a button elsewhere in the application) constructs one,
/// the application's own queue transports it, and `apply` runs it. An
/// application with no such queue reaches the same place through
/// [`DockView::run`](crate::DockView::run).
///
/// **Every op tolerates a stale address.** One is built from a response
/// of the frame before and applied a phase later, by which time the tab,
/// group or split it names may be gone — so an op that resolves to
/// nothing leaves the tree untouched rather than failing.
///
/// Every tab op names its tab by identity, never by strip position: an
/// index would by then address whatever tab slid into that slot.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum DockOp<T> {
    /// Make `tab` visible in whichever group holds it, and focus that
    /// group.
    ActivateTab { tab: T },
    /// Open `tab` in the focused group — reusing it wherever it already
    /// sits — then make it visible and focus its pane.
    OpenTab { tab: T },
    /// Close `tab` wherever it sits. The pinned tab never closes — the
    /// op refuses it.
    CloseTab { tab: T },
    /// Move `tab` to `to` — into another strip, or splitting a pane.
    MoveTab { tab: T, to: DockDrop },
    /// Set the ratio of the split at `split` (its packed root path).
    /// Emitted per frame by a divider drag; coalesces per split.
    SetRatio { split: DockPath, ratio: f32 },
    /// Move focus onto `group`, because a press landed inside its pane.
    FocusPane { group: TabGroupId },
}
