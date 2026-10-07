//! What separates one record pass's interned text from the next's.

use crate::common::id_counter::IdCounter;

/// Identity of one record pass's text arena.
///
/// From a process-wide counter, so one comparison separates passes and windows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct TextEpoch(u64);

impl TextEpoch {
    /// The next unused epoch; taken once per record-pass reset.
    pub(crate) fn reserve() -> Self {
        static NEXT: IdCounter = IdCounter::new();
        Self(NEXT.reserve())
    }
}
