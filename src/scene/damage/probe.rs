//! Observability for the damage diff.
//!
//! Same pattern as [`crate::layout::probe::LayoutProbe`], whose module doc
//! explains it: gated fields, unconditional mutators, so the diff walk
//! carries no `#[cfg]` of its own.
//!
//! This one is `cfg(any(test, feature = "internals"))` rather than
//! test-only because the `damage` bench asserts against the counters. It
//! can afford that where the layout probe cannot — nothing here pushes to
//! a `Vec` on a steady-state frame, so it doesn't perturb the alloc benches.

use crate::scene::tree::record::NodeId;

/// What the diff walk did this pass.
///
/// Reset by [`Self::begin_pass`] at the top of every `compute`, so the
/// counts describe one pass rather than accumulating.
#[derive(Debug, Default)]
pub(crate) struct DamageProbe {
    /// Nodes whose paint rows the diff re-read — the ones that actually
    /// changed. Tests assert both the count and the identities.
    #[cfg(any(test, feature = "internals"))]
    dirty: Vec<NodeId>,
    /// Whole-subtree skips taken. The headline steady-state metric: a
    /// tree that skips at the root does one of these and nothing else.
    #[cfg(any(test, feature = "internals"))]
    subtree_skips: u32,
}

impl DamageProbe {
    /// Clear both counters for a new pass, retaining `dirty`'s capacity.
    #[inline]
    pub(crate) fn begin_pass(&mut self) {
        #[cfg(any(test, feature = "internals"))]
        {
            self.dirty.clear();
            self.subtree_skips = 0;
        }
    }

    #[inline]
    pub(crate) fn mark_dirty(&mut self, #[allow(unused_variables)] node: NodeId) {
        #[cfg(any(test, feature = "internals"))]
        {
            self.dirty.push(node);
        }
    }

    /// Record a subtree skip covering `span` nodes.
    ///
    /// Takes the span rather than being called conditionally because only
    /// a skip of more than one node is interesting — a `span == 1` "skip"
    /// covers just the node itself and would drown the metric. Keeping
    /// that rule here means the walk calls this unconditionally instead of
    /// wrapping it in the test-shaped `if` it used to.
    #[inline]
    pub(crate) fn subtree_skipped(&mut self, #[allow(unused_variables)] span: usize) {
        #[cfg(any(test, feature = "internals"))]
        if span > 1 {
            self.subtree_skips += 1;
        }
    }
}

/// Reads are gated: only tests and benches ask. Not every accessor has
/// both consumers — `subtree_skips` is the bench's headline metric while
/// `dirty` is asserted only by tests — so an `internals`-without-`test`
/// build legitimately leaves one unused.
#[cfg(any(test, feature = "internals"))]
#[allow(dead_code)]
impl DamageProbe {
    pub(crate) fn dirty(&self) -> &[NodeId] {
        &self.dirty
    }

    pub(crate) fn subtree_skips(&self) -> u32 {
        self.subtree_skips
    }
}
