//! What separates one record pass's interned text from the next's.

use crate::common::id_counter::IdCounter;

/// Identity of one record pass's text arena.
///
/// From a process-wide counter, not one per host: an [`InternedStr`](crate::InternedStr) is a public `Copy`
/// handle an app can carry into another host's window, and per-host counters would let its epoch match there
/// and resolve foreign bytes instead of panicking.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct TextEpoch(u64);

impl TextEpoch {
    /// The next unused epoch; taken once per record-pass reset.
    pub(crate) fn reserve() -> Self {
        static NEXT: IdCounter = IdCounter::new();
        Self(NEXT.reserve())
    }
}
