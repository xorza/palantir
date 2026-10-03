//! What separates one record pass's interned text from the next's.

use crate::common::id_counter::IdCounter;

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
    /// counter is nowhere near a hot path.
    pub(crate) fn reserve() -> Self {
        static NEXT: IdCounter = IdCounter::new();
        Self(NEXT.reserve())
    }
}
