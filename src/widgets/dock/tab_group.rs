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
    pub id: TabGroupId,
    /// Non-empty; a group whose last tab closes collapses out of the
    /// tree.
    pub tabs: Vec<T>,
    /// Index of the visible tab; always in range.
    pub active: usize,
}

impl<T: Copy> TabGroup<T> {
    pub fn active_tab(&self) -> T {
        self.tabs[self.active]
    }
}

impl<T> TabGroup<T> {
    /// Remove the tab at `index`, keeping `active` on a surviving slot.
    pub(crate) fn remove_tab(&mut self, index: usize) {
        self.tabs.remove(index);
        self.clamp_active();
    }

    pub(crate) fn clamp_active(&mut self) {
        self.active = self.active.min(self.tabs.len().saturating_sub(1));
    }
}
