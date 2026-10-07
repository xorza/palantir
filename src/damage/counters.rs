//! Observability for the damage diff. `subtree_skips` is the `damage` bench's headline metric ([`BenchOnly`]);
//! `dirty` allocates and only tests read it, so it is [`TestOnly`] (else the alloc suite would measure the probe).

use crate::common::counters::{BenchOnly, TestOnly};
use crate::scene::tree::node_id::NodeId;

/// What the diff walk did this pass, reset by [`Self::begin_pass`] at the top of every `compute`.
#[derive(Debug, Default)]
pub(crate) struct DamageCounters {
    /// Nodes whose paint rows the diff re-read (the ones that changed); only tests read it.
    dirty: TestOnly<Vec<NodeId>>,
    /// Whole-subtree skips taken: the headline steady-state metric.
    subtree_skips: BenchOnly<u32>,
}

impl DamageCounters {
    #[inline]
    pub(crate) fn begin_pass(&mut self) {
        self.dirty.clear();
        self.subtree_skips.reset();
    }

    #[inline]
    pub(crate) fn mark_dirty(&mut self, node: NodeId) {
        self.dirty.push(node);
    }

    /// Records a subtree skip of `span` nodes; only more than one counts (`span == 1` would drown the metric).
    #[inline]
    pub(crate) fn subtree_skipped(&mut self, span: usize) {
        if span > 1 {
            self.subtree_skips.bump();
        }
    }
}

/// Reads are gated with their callers: tests assert `dirty`, `subtree_skips` is also the bench's metric.
#[cfg(test)]
impl DamageCounters {
    pub(crate) fn dirty(&self) -> &[NodeId] {
        self.dirty.as_slice()
    }
}

#[cfg(any(test, feature = "bench"))]
impl DamageCounters {
    pub(crate) const fn subtree_skips(&self) -> u32 {
        self.subtree_skips.count()
    }
}
