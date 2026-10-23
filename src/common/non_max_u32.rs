//! A `u32` that is never `u32::MAX`, so `Option<NonMaxU32>` is four bytes.

use std::num::NonZeroU32;

/// A `u32` below `u32::MAX`, stored inverted so `None` takes the niche of `u32::MAX`: `Option<Self>` is four bytes, and a column of `None` is zeroed memory.
///
/// No `Ord`: the stored order is the reverse of the value order, so compare [`Self::get`].
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct NonMaxU32(NonZeroU32);

impl NonMaxU32 {
    #[inline]
    pub(crate) const fn new(value: u32) -> Option<Self> {
        match NonZeroU32::new(!value) {
            Some(inverted) => Some(Self(inverted)),
            None => None,
        }
    }

    /// `value`, or `u32::MAX - 1` for `u32::MAX`.
    #[inline]
    pub(crate) const fn saturating(value: u32) -> Self {
        match NonZeroU32::new(!value) {
            Some(inverted) => Self(inverted),
            None => Self(NonZeroU32::MIN),
        }
    }

    /// The row `idx` of a table.
    ///
    /// # Panics
    ///
    /// Panics if `idx` is `u32::MAX` or above.
    #[inline]
    pub(crate) fn index(idx: usize) -> Self {
        match u32::try_from(idx).ok().and_then(Self::new) {
            Some(index) => index,
            None => non_max_overflow(idx),
        }
    }

    #[inline]
    pub(crate) const fn get(self) -> u32 {
        !self.0.get()
    }

    #[inline]
    pub(crate) const fn idx(self) -> usize {
        self.get() as usize
    }

    /// Reads back [`to_raw`].
    #[inline]
    pub(crate) const fn from_raw(raw: u32) -> Option<Self> {
        match NonZeroU32::new(raw) {
            Some(raw) => Some(Self(raw)),
            None => None,
        }
    }
}

/// The encoding of `value`, zero for `None`, which [`NonMaxU32::from_raw`] reads back; for a link kept in a foreign `u32`.
#[inline]
pub(crate) const fn to_raw(value: Option<NonMaxU32>) -> u32 {
    match value {
        Some(value) => value.0.get(),
        None => 0,
    }
}

#[cold]
#[inline(never)]
fn non_max_overflow(idx: usize) -> ! {
    panic!("index {idx} is above the u32 index ceiling")
}

#[cfg(test)]
mod tests {
    use crate::common::non_max_u32;
    use crate::common::non_max_u32::NonMaxU32;

    /// Zero and `LAST` round-trip; `u32::MAX` is the `None` niche, stored as zero bits.
    #[test]
    fn non_max_keeps_every_value_below_max() {
        let zero = NonMaxU32::new(0).unwrap();
        let last = NonMaxU32::new(u32::MAX - 1).unwrap();

        assert_eq!(zero.get(), 0);
        assert_eq!(last.get(), u32::MAX - 1);
        assert_eq!(NonMaxU32::new(u32::MAX), None);
        assert_eq!(NonMaxU32::saturating(u32::MAX), last);
        assert_eq!(NonMaxU32::saturating(7).get(), 7);
        assert_eq!(NonMaxU32::index(41).idx(), 41);
        // `!0` is `u32::MAX`, `!(u32::MAX - 1)` is 1.
        assert_eq!(non_max_u32::to_raw(Some(zero)), u32::MAX);
        assert_eq!(non_max_u32::to_raw(Some(last)), 1);
        assert_eq!(non_max_u32::to_raw(None), 0);
        for value in [Some(zero), Some(last), None] {
            assert_eq!(NonMaxU32::from_raw(non_max_u32::to_raw(value)), value);
        }
        assert_eq!(size_of::<Option<NonMaxU32>>(), 4);
    }

    #[test]
    #[should_panic(expected = "index 4294967295 is above the u32 index ceiling")]
    fn non_max_rejects_the_reserved_maximum() {
        let _ = NonMaxU32::index(u32::MAX as usize);
    }
}
