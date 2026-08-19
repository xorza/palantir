//! What separates one record pass's interned text from the next's.

use std::sync::atomic::{AtomicU64, Ordering};

/// Identity of one record pass's text arena.
///
/// Drawn from a process-wide counter, so it separates passes *and*
/// windows with one comparison — a handle from another window can no
/// more match than one from another frame, and neither needs a second
/// field to say so.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct TextEpoch(u64);

impl TextEpoch {
    /// The next unused epoch. Taken once per record-pass reset, so the
    /// atomic is nowhere near a hot path.
    pub(crate) fn next() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        Self(NEXT.fetch_add(1, Ordering::Relaxed))
    }
}
