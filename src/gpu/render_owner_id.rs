//! Stable identity of one window's submitted render stream.

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct RenderOwnerId(u64);

/// The source of one host's [`RenderOwnerId`]s, held by the host's `CpuCore`, which opens all
/// its windows: the ids scope that host's backend, which serves them all.
#[derive(Debug, Default)]
pub(crate) struct RenderOwners {
    next: u64,
}

impl RenderOwners {
    /// The next unused id.
    pub(crate) const fn reserve(&mut self) -> RenderOwnerId {
        let id = self.next;
        self.next += 1;
        RenderOwnerId(id)
    }
}
