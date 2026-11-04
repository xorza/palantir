//! [`NonMaxU16`] and [`NonMaxU32`]: an unsigned index that is never its type's maximum, so
//! `Option<Self>` is as wide as the integer.

use std::num::{NonZeroU16, NonZeroU32};

/// One niche-index type over `$int`: the value stored inverted, so `None` takes the niche of
/// `$int::MAX`, `Option<Self>` is as wide as `$int`, and a column of `None` is zeroed memory. A
/// macro, as std's `NonZero<T>` cannot be named generically on stable; one encoding and one API
/// for both widths. Refused: the 16-bit index stored as index plus one beside the inverted 32-bit
/// one, two encodings of one idea with their raw reads shaped apart.
macro_rules! non_max {
    ($(#[$meta:meta])* $name:ident, $int:ty, $nonzero:ty) => {
        $(#[$meta])*
        ///
        /// No `Ord`: the stored order is the reverse of the value order, so compare [`Self::get`].
        #[repr(transparent)]
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        pub(crate) struct $name($nonzero);

        impl $name {
            /// The highest value held, one below the integer's maximum.
            pub(crate) const LAST: $int = <$int>::MAX - 1;

            #[inline]
            pub(crate) const fn new(value: $int) -> Option<Self> {
                match <$nonzero>::new(!value) {
                    Some(inverted) => Some(Self(inverted)),
                    None => None,
                }
            }

            /// The row `idx` of a table.
            ///
            /// # Panics
            ///
            /// Panics if `idx` is past [`Self::LAST`], at the caller's site, which names the
            /// table that filled.
            #[inline]
            #[track_caller]
            pub(crate) fn index(idx: usize) -> Self {
                match <$int>::try_from(idx).ok().and_then(Self::new) {
                    Some(index) => index,
                    None => index_overflow(idx, u64::from(Self::LAST)),
                }
            }

            #[inline]
            pub(crate) const fn get(self) -> $int {
                !self.0.get()
            }

            #[inline]
            pub(crate) const fn idx(self) -> usize {
                self.get() as usize
            }

            /// The encoding a foreign field of the integer holds, which [`Self::from_raw`] reads
            /// back: the stored bits, and zero for `None`.
            #[inline]
            pub(crate) const fn to_raw(index: Option<Self>) -> $int {
                match index {
                    Some(index) => index.0.get(),
                    None => 0,
                }
            }

            /// Reads back [`Self::to_raw`]; `None` for zero.
            #[inline]
            pub(crate) const fn from_raw(raw: $int) -> Option<Self> {
                match <$nonzero>::new(raw) {
                    Some(raw) => Some(Self(raw)),
                    None => None,
                }
            }
        }
    };
}

non_max!(
    /// A two-byte index below `u16::MAX`: a plane's grid and scrollbar definitions, whose ids
    /// pack into a 16-bit layout payload. [`Self::LAST`] is a real ceiling, which a table can
    /// reach.
    NonMaxU16,
    u16,
    NonZeroU16
);

non_max!(
    /// A four-byte index below `u32::MAX`.
    NonMaxU32,
    u32,
    NonZeroU32
);

impl NonMaxU32 {
    /// `value`, or [`Self::LAST`] for `u32::MAX`.
    #[inline]
    pub(crate) const fn saturating(value: u32) -> Self {
        match Self::new(value) {
            Some(index) => index,
            None => Self(NonZeroU32::MIN),
        }
    }
}

#[cold]
#[inline(never)]
#[track_caller]
fn index_overflow(idx: usize, last: u64) -> ! {
    panic!("index {idx} is past the index ceiling {last}")
}

#[cfg(test)]
mod tests {
    use crate::common::non_max::{NonMaxU16, NonMaxU32};

    /// Zero and `LAST` round-trip; the maximum is the `None` niche, stored as zero bits, and the
    /// raw form of a value is its inverse: `!0` the maximum, `!(MAX - 1)` one.
    #[test]
    fn non_max_keeps_every_value_below_max() {
        let zero = NonMaxU32::new(0).unwrap();
        let last = NonMaxU32::new(NonMaxU32::LAST).unwrap();
        assert_eq!(zero.get(), 0);
        assert_eq!(last.get(), u32::MAX - 1);
        assert_eq!(NonMaxU32::new(u32::MAX), None);
        assert_eq!(NonMaxU32::saturating(u32::MAX), last);
        assert_eq!(NonMaxU32::saturating(7).get(), 7);
        assert_eq!(NonMaxU32::index(41).idx(), 41);
        assert_eq!(NonMaxU32::to_raw(Some(zero)), u32::MAX);
        assert_eq!(NonMaxU32::to_raw(Some(last)), 1);
        assert_eq!(NonMaxU32::to_raw(None), 0);
        assert_eq!(NonMaxU32::from_raw(0), None);
        for value in [Some(zero), Some(last), None] {
            assert_eq!(NonMaxU32::from_raw(NonMaxU32::to_raw(value)), value);
        }
        assert_eq!(size_of::<Option<NonMaxU32>>(), 4);

        let first = NonMaxU16::index(0);
        let last = NonMaxU16::index(65_534);
        assert_eq!((first.idx(), NonMaxU16::to_raw(Some(first))), (0, u16::MAX));
        assert_eq!((last.idx(), NonMaxU16::to_raw(Some(last))), (65_534, 1));
        assert_eq!(NonMaxU16::from_raw(0), None);
        assert_eq!(NonMaxU16::from_raw(u16::MAX), Some(first));
        assert_eq!(size_of::<Option<NonMaxU16>>(), 2);
    }

    #[test]
    #[should_panic(expected = "index 4294967295 is past the index ceiling 4294967294")]
    fn non_max_rejects_the_reserved_maximum() {
        let _ = NonMaxU32::index(u32::MAX as usize);
    }

    /// A 16-bit table fills at 65 535 rows, a ceiling a plane can reach.
    #[test]
    #[should_panic(expected = "index 65535 is past the index ceiling 65534")]
    fn a_two_byte_index_fills_at_its_ceiling() {
        let _ = NonMaxU16::index(65_535);
    }
}
