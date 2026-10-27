//! One pane's tab strip, and the id that addresses it.

use serde::{Deserialize, Serialize};

use crate::widgets::tabs::reindex;

/// A [`TabGroup`]'s identity: the long-lived address of one pane.
///
/// Minted by [`DockState`](crate::DockState) from a counter it keeps, so states built by the same
/// calls carry the same ids and compare equal. Unlike a node index it survives the re-pack every
/// structural operation ends with.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TabGroupId(pub(super) u64);

/// One pane's tab strip: the open tabs plus which one is visible. Read-only outside the crate:
/// [`DockState`](crate::DockState) keeps the strip non-empty and the visible index in range.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TabGroup<T> {
    pub(super) id: TabGroupId,
    pub(super) tabs: Vec<T>,
    pub(super) active: usize,
}

impl<T> TabGroup<T> {
    /// Stable identity; survives the re-pack every structural operation ends with.
    pub const fn id(&self) -> TabGroupId {
        self.id
    }

    /// The open tabs in strip order; never empty, as a group whose last tab closes collapses
    /// out of the tree.
    pub fn tabs(&self) -> &[T] {
        &self.tabs
    }

    /// The visible tab's slot in [`Self::tabs`].
    pub const fn active(&self) -> usize {
        self.active
    }

    /// Removes the tab at `idx`, and keeps showing the tab that was showing; see
    /// [`reindex::after_removal`].
    pub(super) fn remove_tab(&mut self, idx: usize) {
        self.active = reindex::after_removal(self.active, idx, self.tabs.len());
        self.tabs.remove(idx);
    }

    /// [`Self::remove_tab`]'s rule over a whole filter: drops every tab `keep` rejects, and keeps showing
    /// the one that was showing.
    ///
    /// Counted rather than searched (the active tab moves down one slot per rejected tab ahead of it), so
    /// no tab comparison is needed and one pass covers any number of removals.
    pub(super) fn retain(&mut self, mut keep: impl FnMut(&T) -> bool) {
        let mut idx = 0;
        let mut active = self.active;
        self.tabs.retain(|tab| {
            let kept = keep(tab);
            if !kept && idx < active {
                active -= 1;
            }
            idx += 1;
            kept
        });
        // Where the visible tab and every tab after it were rejected, its slot lies past the end
        // and the last kept tab shows.
        self.active = active.min(self.tabs.len().saturating_sub(1));
    }
}

impl<T: Copy> TabGroup<T> {
    /// The visible tab. Always present — a group is never empty.
    pub fn active_tab(&self) -> T {
        self.tabs[self.active]
    }
}
