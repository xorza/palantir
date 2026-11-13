//! [`SeededRng`]: a stream of numbers a seeded test run replays.

/// Marsaglia's xorshift64, so a failing run replays from its seed: not for anything a person sees.
#[derive(Debug)]
pub(crate) struct SeededRng(u64);

impl SeededRng {
    /// The stream from `seed`.
    ///
    /// # Panics
    ///
    /// If `seed` is zero, which xorshift never leaves.
    pub(crate) const fn new(seed: u64) -> Self {
        assert!(seed != 0, "a xorshift seed is not zero");
        Self(seed)
    }

    /// The next number.
    pub(crate) const fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    /// The next number below `n`.
    pub(crate) const fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }

    /// The next number in `lo..=hi`.
    pub(crate) fn within(&mut self, lo: u16, hi: u16) -> u16 {
        lo + u16::try_from(self.below(u64::from(hi - lo) + 1)).expect("below the span")
    }
}
