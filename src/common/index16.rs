//! A two-byte arena index with room for `None`, so `Option<Index16>` is two bytes.

use std::num::NonZeroU16;

/// Arena index whose nonzero encoding keeps `Option<Self>` at two bytes: the stored value is the index plus one.
///
/// **[`Self::LAST`] is a real ceiling.** A table addressed this way holds at most 65 535 rows and a row past that panics in release, naming the table; see [`ExtrasIdx`](crate::scene::tree::extras_idx::ExtrasIdx).
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct Index16(NonZeroU16);

impl Index16 {
    /// The highest index held: one below `u16::MAX`, since the stored value is index plus one.
    pub(crate) const LAST: usize = u16::MAX as usize - 1;

    /// The index of a row just pushed onto `table`; the name makes an overflow report which table filled.
    ///
    /// # Panics
    ///
    /// Panics if `index` is above [`Self::LAST`].
    #[inline]
    pub(crate) fn new(index: usize, table: &'static str) -> Self {
        if index > Self::LAST {
            index16_overflow(index, table);
        }
        Self(NonZeroU16::new(index as u16 + 1).unwrap())
    }

    #[inline]
    pub(crate) const fn idx(self) -> usize {
        self.0.get() as usize - 1
    }

    /// The encoded value (index plus one) that [`Self::from_raw`] reads back.
    pub(crate) const fn to_raw(self) -> u16 {
        self.0.get()
    }

    pub(crate) const fn from_raw(raw: u16) -> Option<Self> {
        match NonZeroU16::new(raw) {
            Some(raw) => Some(Self(raw)),
            None => None,
        }
    }
}

#[cold]
#[inline(never)]
fn index16_overflow(index: usize, table: &'static str) -> ! {
    panic!(
        "{table} exceeded its {} row ceiling at row {index}",
        Index16::LAST + 1,
    )
}

#[cfg(test)]
mod tests {
    use crate::common::index16::Index16;

    #[test]
    fn index16_preserves_boundaries_and_option_niche() {
        let first = Index16::new(0, "test_table");
        let last = Index16::new(65_534, "test_table");

        assert_eq!(first.idx(), 0);
        assert_eq!(first.to_raw(), 1);
        assert_eq!(Index16::from_raw(0), None);
        assert_eq!(last.idx(), 65_534);
        assert_eq!(last.to_raw(), u16::MAX);
        assert_eq!(Index16::from_raw(u16::MAX), Some(last));
        assert_eq!(size_of::<Index16>(), 2);
        assert_eq!(size_of::<Option<Index16>>(), 2);
    }

    /// The overflow names the caller's table, since a bare "Index16" leaves the reader to find which of five.
    #[test]
    #[should_panic(expected = "bounds_table exceeded its 65535 row ceiling at row 65535")]
    fn index16_rejects_reserved_maximum() {
        let _ = Index16::new(65_535, "bounds_table");
    }
}
