//! Stable identity of one window's submitted render stream.

use std::cell::Cell;
use std::rc::Rc;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct RenderOwnerId(u64);

/// The source of one host's [`RenderOwnerId`]s, shared by every clone of its
/// resources: the ids scope that host's backend, which serves all its windows.
#[derive(Clone, Debug, Default)]
pub(crate) struct RenderOwners(Rc<Cell<u64>>);

impl RenderOwners {
    /// The next unused id.
    pub(crate) fn reserve(&self) -> RenderOwnerId {
        let id = self.0.get();
        self.0.set(id + 1);
        RenderOwnerId(id)
    }
}
