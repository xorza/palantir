//! The dock's persisted arrangement, as a file holds it.

use serde::{Deserialize, Serialize};

use crate::widgets::dock::dock_node::DockNode;
use crate::widgets::dock::tab_group::TabGroupId;

/// What a [`DockState`](crate::DockState) persists: the split tree and the focused pane, and
/// nothing a caller sets. Serialize [`DockState::layout`](crate::DockState::layout) to save, and
/// hand a deserialized one to [`DockState::load`](crate::DockState::load), which checks it. The
/// pinned tab and the view's policy are the application's, given again at each load, so a file
/// cannot carry a different one.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DockLayout<T> {
    pub(super) nodes: Vec<DockNode<T>>,
    pub(super) focused: TabGroupId,
    pub(super) next_group: u64,
}
