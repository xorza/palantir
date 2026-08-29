//! A process-wide monotonic counter, for the ids that separate
//! everything ever minted.

use std::sync::atomic::{AtomicU64, Ordering};

/// A process-wide source of monotonically increasing ids.
///
/// The ids built on one are the ones that must not collide across
/// *anything* — two windows, two frames, or two of either alive at once
/// — and that nobody hands around: a window's render owner, a record
/// pass's text epoch. Each declares its own `static` and reads it here,
/// so two counters never share a sequence and no owner has to spell the
/// atomic out again.
///
/// Starts at 1. Every id built on one keeps 0 for its own "no such
/// thing", and skipping it here is what saves each of them from doing so
/// by hand.
#[derive(Debug)]
pub(crate) struct IdCounter(AtomicU64);

impl IdCounter {
    pub(crate) const fn new() -> Self {
        Self(AtomicU64::new(1))
    }

    /// The next unused number.
    ///
    /// `Relaxed`: the counter's whole contract is that no two reads
    /// answer alike, which `fetch_add` guarantees on its own. Nothing is
    /// published through it, so there is no ordering to establish.
    pub(crate) fn reserve(&self) -> u64 {
        self.0.fetch_add(1, Ordering::Relaxed)
    }
}
