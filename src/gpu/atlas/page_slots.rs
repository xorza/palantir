//! [`PageSlots`]: the page slots of a list a draw reads.

use crate::gpu::atlas::page_list::MAX_PAGES;

/// The open page slots of a list, never none: what a pipeline over the list's pages is built for,
/// so its shader picks among the open slots alone, and over one open page loads it with no branch
/// wherever the page sits, as Skia keys its text program on the atlas's active pages
/// (`src/gpu/AGENTS.md`, Atlases). Slot `i` at bit `i`; a list with no page open reads slot 0,
/// its stand-in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PageSlots(u8);

impl PageSlots {
    /// Slot 0 alone.
    pub(crate) const FIRST: Self = Self(1);

    /// The sets there are: every one but the empty.
    pub(crate) const COUNT: usize = (1 << MAX_PAGES) - 1;

    /// The slots whose bits `bits` sets.
    ///
    /// # Panics
    ///
    /// If `bits` sets none, or one past [`MAX_PAGES`].
    pub(crate) const fn new(bits: u8) -> Self {
        assert!(bits != 0, "a draw reads a slot");
        assert!((bits as usize) <= Self::COUNT, "a slot past the list's");
        Self(bits)
    }

    /// Its bits, slot `i` at bit `i`.
    pub(crate) const fn bits(self) -> u32 {
        self.0 as u32
    }

    /// Its place among the sets, from 0, below [`Self::COUNT`].
    pub(crate) const fn idx(self) -> usize {
        self.0 as usize - 1
    }
}

#[cfg(test)]
pub(crate) mod internals {
    use crate::gpu::atlas::page_slots::PageSlots;

    impl PageSlots {
        /// Every set, in [`Self::idx`] order.
        pub(crate) fn all() -> impl Iterator<Item = Self> {
            (1..=Self::COUNT as u8).map(Self::new)
        }

        /// Whether it holds slot `page`.
        pub(crate) const fn holds(self, page: u8) -> bool {
            self.0 & (1 << page) != 0
        }
    }
}
