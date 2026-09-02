//! What a dock addresses its tabs by.

use std::fmt::Debug;
use std::hash::Hash;

/// The bound a [`DockState`](crate::DockState) tab key carries: a small
/// copyable value that names one tab and nothing else.
///
/// **A key, not the content.** The application resolves it into a title
/// and a body every frame, through
/// [`DockTabs`](crate::DockTabs) — which is why the tree stays
/// `Clone + PartialEq + Serialize` and an undo layer can diff two
/// snapshots for a no-op. Storing the tab *value* in the tree instead
/// would force `&mut Tab` through every viewer method and make
/// structural equality mean whatever the payload's `PartialEq` means.
///
/// Blanket-implemented, so an application names its own enum and
/// nothing else.
pub trait DockTab: Copy + Eq + Hash + Debug + 'static {}

impl<T: Copy + Eq + Hash + Debug + 'static> DockTab for T {}
