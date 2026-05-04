use std::ops::Range;

/// `(start, len)` index range over a flat arena. Compact — 8 bytes —
/// because measure-cache snapshots and grid hug slots store many of
/// these and we want to keep the per-entry footprint small.
///
/// Convertible to/from `Range<u32>` via `From` — use that for
/// constructing a `Span` from a `start..end` literal or for handing one
/// to a u32-range-taking API. `range()` returns `Range<usize>` for
/// slicing into `Vec<T>`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Span {
    pub(crate) start: u32,
    pub(crate) len: u32,
}

impl Span {
    #[inline]
    pub(crate) fn new(start: u32, len: u32) -> Self {
        Self { start, len }
    }

    #[inline]
    pub(crate) fn range(self) -> Range<usize> {
        self.start as usize..(self.start + self.len) as usize
    }
}

impl From<Range<u32>> for Span {
    #[inline]
    fn from(r: Range<u32>) -> Self {
        Self {
            start: r.start,
            len: r.end - r.start,
        }
    }
}

impl From<Range<usize>> for Span {
    #[inline]
    fn from(r: Range<usize>) -> Self {
        Self {
            start: r.start as u32,
            len: (r.end - r.start) as u32,
        }
    }
}

impl From<Span> for Range<u32> {
    #[inline]
    fn from(s: Span) -> Self {
        s.start..s.start + s.len
    }
}

impl From<Span> for Range<usize> {
    #[inline]
    fn from(s: Span) -> Self {
        s.start as usize..(s.start + s.len) as usize
    }
}
