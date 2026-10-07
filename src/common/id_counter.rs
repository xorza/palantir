//! A process-wide monotonic id counter.

use std::sync::atomic::{AtomicU64, Ordering};

/// A process-wide source of increasing ids, for ids that must not collide across windows or frames.
///
/// Starts at 1 so 0 stays free as each id's "none".
#[derive(Debug)]
pub(crate) struct IdCounter(AtomicU64);

impl IdCounter {
    pub(crate) const fn new() -> Self {
        Self(AtomicU64::new(1))
    }

    /// The next unused number.
    ///
    /// `Relaxed`: `fetch_add` alone makes reads distinct; nothing is published through it.
    pub(crate) fn reserve(&self) -> u64 {
        self.0.fetch_add(1, Ordering::Relaxed)
    }
}
