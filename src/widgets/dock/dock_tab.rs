//! What a dock addresses its tabs by.

use std::fmt::Debug;
use std::hash::Hash;

/// The bound a [`DockState`](crate::DockState) tab key carries: a small copyable value naming one tab.
///
/// A key, not the content: the application resolves it into a title and body every frame through [`DockTabs`](crate::DockTabs), so the tree stays `Clone + PartialEq + Serialize` and an undo layer can diff snapshots. Blanket-implemented; an application names only its own enum.
pub trait DockTab: Copy + Eq + Hash + Debug + 'static {}

impl<T: Copy + Eq + Hash + Debug + 'static> DockTab for T {}
