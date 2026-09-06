//! One pane's tab strip, and the id that addresses it.

use serde::{Deserialize, Serialize};

/// A [`TabGroup`]'s identity — the long-lived address of one pane.
///
/// Minted by [`DockState`](crate::DockState) from a counter it keeps, so
/// two states built by the same sequence of calls carry the same ids and
/// compare equal. Unlike a node index it survives the re-pack every
/// structural op ends with.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TabGroupId(pub(crate) u64);

/// One pane's tab strip: the open tabs plus which one is visible.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TabGroup<T> {
    /// Stable identity, which survives the re-pack every structural op
    /// ends with.
    pub id: TabGroupId,
    /// Non-empty; a group whose last tab closes collapses out of the
    /// tree.
    pub tabs: Vec<T>,
    /// Index of the visible tab; always in range.
    pub active: usize,
}

impl<T: Copy> TabGroup<T> {
    /// The visible tab. Always present — a group is never empty.
    pub fn active_tab(&self) -> T {
        self.tabs[self.active]
    }
}

impl<T> TabGroup<T> {
    /// Remove the tab at `index`, and keep showing the tab that was
    /// showing.
    ///
    /// A strip is addressed by index and read by identity: removing a
    /// tab ahead of the visible one slides it down a slot, so an index
    /// held still would switch the pane to a document nobody asked for.
    /// Removing the visible tab is the one case that chooses another,
    /// and it chooses the tab that took the slot — or the one before it,
    /// where the slot was the last.
    pub(crate) fn remove_tab(&mut self, index: usize) {
        self.tabs.remove(index);
        if index < self.active {
            self.active -= 1;
        }
        self.clamp_active();
    }

    /// [`Self::remove_tab`]'s rule over a whole filter: drop every tab
    /// `keep` rejects, and keep showing the one that was showing.
    ///
    /// Counted rather than searched — the active tab moves down one slot
    /// per rejected tab ahead of it — so this needs no way to compare
    /// two tabs, and one pass answers for any number of removals.
    pub(crate) fn retain(&mut self, mut keep: impl FnMut(&T) -> bool) {
        let mut index = 0;
        let mut active = self.active;
        self.tabs.retain(|tab| {
            let kept = keep(tab);
            if !kept && index < active {
                active -= 1;
            }
            index += 1;
            kept
        });
        self.active = active;
        self.clamp_active();
    }

    /// Pull the visible index back into range after the strip shrank.
    /// The tail case both removers land in, and the only one their own
    /// arithmetic cannot answer.
    fn clamp_active(&mut self) {
        self.active = self.active.min(self.tabs.len().saturating_sub(1));
    }
}
