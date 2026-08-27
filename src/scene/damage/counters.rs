//! Observability for the damage diff. Built on [`BenchOnly`], whose module
//! doc explains the gated-cell pattern and why the two gates exist.
//!
//! The two cells take different gates, because they answer to different
//! callers. `subtree_skips` is the `damage` bench's headline metric, so
//! it is [`BenchOnly`]; `dirty` is a `Vec` cell only tests read, and the
//! module rule puts anything that allocates on
//! [`TestOnly`] — the alloc suite
//! asserts steady-state frames allocate nothing and would otherwise
//! measure the probe. It would have been affordable either way (`dirty`
//! pushes only on a node that actually changed, so a steady-state frame
//! appends nothing), but affordable is not a reason to widen a gate.

use crate::common::counters::{BenchOnly, TestOnly};
use crate::scene::tree::node_id::NodeId;

/// What the diff walk did this pass.
///
/// Reset by [`Self::pre_record`] at the top of every `compute`, so the
/// counts describe one pass rather than accumulating.
#[derive(Debug, Default)]
pub(crate) struct DamageCounters {
    /// Nodes whose paint rows the diff re-read — the ones that actually
    /// changed. Tests assert both the count and the identities, and
    /// nothing else asks — so this one takes the narrow gate, which is
    /// also the gate the module rule demands of a cell that pushes to a
    /// `Vec`.
    dirty: TestOnly<Vec<NodeId>>,
    /// Whole-subtree skips taken. The headline steady-state metric: a
    /// tree that skips at the root does one of these and nothing else.
    subtree_skips: BenchOnly<u32>,
}

impl DamageCounters {
    /// Clear both counters for a new pass, retaining `dirty`'s capacity.
    #[inline]
    pub(crate) fn pre_record(&mut self) {
        self.dirty.clear();
        self.subtree_skips.reset();
    }

    #[inline]
    pub(crate) fn mark_dirty(&mut self, node: NodeId) {
        self.dirty.push(node);
    }

    /// Record a subtree skip covering `span` nodes.
    ///
    /// Takes the span rather than being called conditionally because only
    /// a skip of more than one node is interesting — a `span == 1` "skip"
    /// covers just the node itself and would drown the metric. Keeping
    /// that rule here lets the walk call this unconditionally rather than
    /// wrap it in a test-shaped `if` at the call site.
    #[inline]
    pub(crate) fn subtree_skipped(&mut self, span: usize) {
        if span > 1 {
            self.subtree_skips.bump();
        }
    }
}

/// Reads are gated with their callers, one gate each rather than one
/// wide gate and an `allow(dead_code)`: `dirty` is asserted only by
/// tests, while `subtree_skips` is also the `damage` bench's headline
/// metric.
#[cfg(test)]
impl DamageCounters {
    pub(crate) fn dirty(&self) -> &[NodeId] {
        self.dirty.as_slice()
    }
}

#[cfg(any(test, feature = "bench"))]
impl DamageCounters {
    pub(crate) fn subtree_skips(&self) -> u32 {
        self.subtree_skips.count()
    }
}
