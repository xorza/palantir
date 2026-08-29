//! A two-byte arena index whose encoding leaves room for `None`, so an
//! optional one still fits in two bytes.

use std::num::NonZeroU16;

/// Arena index whose nonzero encoding keeps `Option<Self>` at two bytes.
///
/// The stored value is one greater than the index, leaving zero for `None`.
///
/// **[`Self::LAST`] is a real ceiling on the table, not an internal
/// detail.** Every table addressed this way holds at most 65 535 rows, and
/// a row past that panics in release. Each caller names its table, so the
/// panic says which one filled up rather than only that some `Index16`
/// did — see [`ExtrasIdx`](crate::scene::tree::extras_idx::ExtrasIdx),
/// which documents what fills the three it addresses.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct Index16(NonZeroU16);

impl Index16 {
    /// The highest index this can hold. One below `u16::MAX` because the
    /// stored value is the index plus one.
    pub(crate) const LAST: usize = u16::MAX as usize - 1;

    /// The index of a row just pushed onto `table`, named so an overflow
    /// reports which table filled up.
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
    pub(crate) fn idx(self) -> usize {
        self.0.get() as usize - 1
    }

    pub(crate) const fn from_raw(raw: u16) -> Option<Self> {
        match NonZeroU16::new(raw) {
            Some(raw) => Some(Self(raw)),
            None => None,
        }
    }
}

impl From<Index16> for u16 {
    fn from(value: Index16) -> Self {
        value.0.get()
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
        assert_eq!(u16::from(first), 1);
        assert_eq!(Index16::from_raw(0), None);
        assert_eq!(last.idx(), 65_534);
        assert_eq!(u16::from(last), u16::MAX);
        assert_eq!(Index16::from_raw(u16::MAX), Some(last));
        assert_eq!(std::mem::size_of::<Index16>(), 2);
        assert_eq!(std::mem::size_of::<Option<Index16>>(), 2);
    }

    /// The overflow names the table that filled up: the ceiling is a
    /// property of the caller's arena, and a message that only says
    /// "Index16" leaves the reader to find which of five tables it was.
    #[test]
    #[should_panic(expected = "bounds_table exceeded its 65535 row ceiling at row 65535")]
    fn index16_rejects_reserved_maximum() {
        let _ = Index16::new(65_535, "bounds_table");
    }
}
