//! Observability for the cascade pass, built on [`TestOnly`] (see its module doc for the gated-cell pattern).
//!
//! Accumulate is the right default: [`CascadeEngine::run`] skips on an unchanged key, so a per-pass reset would never fire and those frames would report the previous run's numbers.
//!
//! [`CascadeEngine::run`]: crate::cascade::engine::CascadeEngine

use crate::common::counters::TestOnly;

/// What the cascade did, for tests. Every path ends in the same cascade, so these are what tell a rebuild, a repack the key foresaw and a patch that gave up halfway apart.
#[derive(Debug, Default)]
pub(crate) struct CascadeCounters {
    /// Full rebuilds performed.
    full_rebuilds: TestOnly<u32>,
    /// Incremental walks that gave up partway, so their plane was repacked after them.
    abandoned_incrementals: TestOnly<u32>,
    /// Planes whose paint arena a refresh laid out afresh, keeping the structural tables.
    repacks: TestOnly<u32>,
    /// Nodes an incremental walk recomputed rather than kept; tells a repair inside the changed subtree from a full-tree walk.
    refreshed_nodes: TestOnly<u32>,
    /// Whether the last run did work rather than skip on an unchanged key.
    ran: TestOnly<bool>,
}

impl CascadeCounters {
    #[inline]
    pub(crate) fn full_rebuild(&mut self) {
        self.full_rebuilds.bump();
    }

    #[inline]
    pub(crate) fn abandoned_incremental(&mut self) {
        self.abandoned_incrementals.bump();
    }

    #[inline]
    pub(crate) fn repack(&mut self) {
        self.repacks.bump();
    }

    #[inline]
    pub(crate) fn refreshed_node(&mut self) {
        self.refreshed_nodes.bump();
    }

    #[inline]
    pub(crate) fn note_ran(&mut self, ran: bool) {
        self.ran.edit(|noted| *noted = ran);
    }
}

/// The probe's reads, for the crate's tests; gating them lets the counters be absent from shipping builds.
#[cfg(test)]
pub(crate) mod internals {
    use crate::cascade::counters::CascadeCounters;

    impl CascadeCounters {
        pub(crate) fn full_rebuilds(&self) -> u32 {
            self.full_rebuilds.count()
        }

        pub(crate) fn abandoned_incrementals(&self) -> u32 {
            self.abandoned_incrementals.count()
        }

        pub(crate) fn repacks(&self) -> u32 {
            self.repacks.count()
        }

        pub(crate) fn refreshed_nodes(&self) -> u32 {
            self.refreshed_nodes.count()
        }

        pub(crate) fn ran(&self) -> bool {
            *self.ran.get()
        }
    }
}
