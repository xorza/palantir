//! [`BitSet`]: a growable set of small indices, a bit each, as `fixedbitset` keeps one, with the
//! calls palantir makes.

use std::ops::Range;

const BITS: usize = u64::BITS as usize;

/// A set of indices below its length, one bit each in 64-bit blocks. It grows on request and
/// never shrinks, so a set reused each frame keeps its blocks.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct BitSet {
    blocks: Vec<u64>,
    len: usize,
}

impl BitSet {
    /// An empty set holding no index.
    pub(crate) const fn new() -> Self {
        Self {
            blocks: Vec::new(),
            len: 0,
        }
    }

    /// Hold indices up to `len`, the new ones out of the set; a smaller `len` changes nothing.
    pub(crate) fn grow(&mut self, len: usize) {
        if len > self.len {
            self.len = len;
            self.blocks.resize(len.div_ceil(BITS), 0);
        }
    }

    /// Take every index out of the set, keeping its length.
    pub(crate) fn clear(&mut self) {
        self.blocks.fill(0);
    }

    /// Whether `idx` is in the set; an index past its length never is.
    pub(crate) fn contains(&self, idx: usize) -> bool {
        idx < self.len && self.blocks[idx / BITS] & bit(idx) != 0
    }

    /// Put `idx` in the set.
    ///
    /// # Panics
    ///
    /// Panics if `idx` is not below its length.
    #[track_caller]
    pub(crate) fn insert(&mut self, idx: usize) {
        self.put(idx);
    }

    /// Put `idx` in the set, and say whether it was in already.
    ///
    /// # Panics
    ///
    /// Panics if `idx` is not below its length.
    #[track_caller]
    pub(crate) fn put(&mut self, idx: usize) -> bool {
        assert!(idx < self.len, "index {idx} past a set of {}", self.len);
        let block = &mut self.blocks[idx / BITS];
        let was = *block & bit(idx) != 0;
        *block |= bit(idx);
        was
    }

    /// Take every index of `range` out of the set.
    ///
    /// # Panics
    ///
    /// Panics if `range` ends past its length.
    #[track_caller]
    pub(crate) fn remove_range(&mut self, range: Range<usize>) {
        assert!(
            range.end <= self.len,
            "range {range:?} past a set of {}",
            self.len
        );
        let mut at = range.start;
        while at < range.end {
            let start = at % BITS;
            let end = (range.end - (at - start)).min(BITS);
            self.blocks[at / BITS] &= !span(start, end);
            at += end - start;
        }
    }

    /// The indices in the set, ascending.
    pub(crate) fn ones(&self) -> Ones<'_> {
        Ones {
            blocks: &self.blocks,
            base: 0,
            current: self.blocks.first().copied().unwrap_or(0),
        }
    }
}

/// The bit of `idx` in its block.
const fn bit(idx: usize) -> u64 {
    1 << (idx % BITS)
}

/// The bits `start..end` of a block, `end <= 64`.
const fn span(start: usize, end: usize) -> u64 {
    let below_end = if end == BITS {
        u64::MAX
    } else {
        (1 << end) - 1
    };
    below_end & !((1 << start) - 1)
}

/// The indices of a [`BitSet`], ascending; [`BitSet::ones`] hands it out.
#[derive(Clone, Debug)]
pub(crate) struct Ones<'a> {
    blocks: &'a [u64],
    /// The index of the current block's first bit.
    base: usize,
    /// The current block's bits not yet handed out.
    current: u64,
}

impl Iterator for Ones<'_> {
    type Item = usize;

    fn next(&mut self) -> Option<usize> {
        while self.current == 0 {
            let next = self.base / BITS + 1;
            self.current = *self.blocks.get(next)?;
            self.base = next * BITS;
        }
        let at = self.current.trailing_zeros() as usize;
        self.current &= self.current - 1;
        Some(self.base + at)
    }
}

#[cfg(test)]
mod tests {
    use crate::common::bit_set::BitSet;

    /// A set holds what goes in and hands it back ascending across block edges; `put` tells a
    /// repeat; `remove_range` clears inside one block, across several and to the end; `clear`
    /// keeps the length; a smaller `grow` changes nothing; an index past the length is out.
    #[test]
    fn a_set_holds_its_indices_across_blocks() {
        let mut set = BitSet::new();
        assert_eq!(set.ones().next(), None);
        set.grow(200);
        for idx in [0, 1, 63, 64, 65, 127, 128, 199] {
            set.insert(idx);
        }
        assert!(!set.put(5), "5 was out");
        assert!(set.put(5), "5 is in now");
        assert_eq!(
            set.ones().collect::<Vec<_>>(),
            [0, 1, 5, 63, 64, 65, 127, 128, 199]
        );
        assert!(set.contains(64));
        assert!(!set.contains(66));
        assert!(!set.contains(500), "past the length");

        set.remove_range(1..6);
        assert_eq!(
            set.ones().collect::<Vec<_>>(),
            [0, 63, 64, 65, 127, 128, 199]
        );
        set.remove_range(63..128);
        assert_eq!(set.ones().collect::<Vec<_>>(), [0, 128, 199]);
        set.remove_range(128..200);
        set.remove_range(10..10);
        assert_eq!(set.ones().collect::<Vec<_>>(), [0]);

        set.grow(100);
        set.clear();
        assert_eq!(set.ones().next(), None);
        set.insert(199);
        assert_eq!(
            set.ones().collect::<Vec<_>>(),
            [199],
            "a smaller grow and a clear keep 200"
        );
        set.grow(256);
        set.insert(255);
        assert_eq!(set.ones().collect::<Vec<_>>(), [199, 255]);
    }

    /// An insert past the length is refused.
    #[test]
    #[should_panic(expected = "index 8 past a set of 8")]
    fn an_insert_past_the_length_is_refused() {
        let mut set = BitSet::new();
        set.grow(8);
        set.insert(8);
    }
}
