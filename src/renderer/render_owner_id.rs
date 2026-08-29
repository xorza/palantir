//! Stable identity of one window's submitted render stream.

use crate::common::id_counter::IdCounter;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct RenderOwnerId(u64);

impl RenderOwnerId {
    pub(crate) fn reserve() -> Self {
        static NEXT: IdCounter = IdCounter::new();
        Self(NEXT.reserve())
    }
}
