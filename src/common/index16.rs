use std::num::NonZeroU16;

/// Arena index whose nonzero encoding keeps `Option<Self>` at two bytes.
///
/// The stored value is one greater than the index, leaving zero for `None`.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct Index16(NonZeroU16);

impl Index16 {
    const LAST: usize = u16::MAX as usize - 1;

    #[inline]
    pub(crate) fn new(index: usize) -> Self {
        if index > Self::LAST {
            index16_overflow(index);
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
fn index16_overflow(index: usize) -> ! {
    panic!(
        "Index16 value {index} exceeds the last representable index {}",
        Index16::LAST,
    )
}

#[cfg(test)]
mod tests {
    use crate::common::index16::Index16;

    #[test]
    fn index16_preserves_boundaries_and_option_niche() {
        let first = Index16::new(0);
        let last = Index16::new(65_534);

        assert_eq!(first.idx(), 0);
        assert_eq!(u16::from(first), 1);
        assert_eq!(Index16::from_raw(0), None);
        assert_eq!(last.idx(), 65_534);
        assert_eq!(u16::from(last), u16::MAX);
        assert_eq!(Index16::from_raw(u16::MAX), Some(last));
        assert_eq!(std::mem::size_of::<Index16>(), 2);
        assert_eq!(std::mem::size_of::<Option<Index16>>(), 2);
    }

    #[test]
    #[should_panic(expected = "Index16 value 65535 exceeds the last representable index 65534")]
    fn index16_rejects_reserved_maximum() {
        let _ = Index16::new(65_535);
    }
}
