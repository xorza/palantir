//! A `(start, len)` range into a flat arena, in eight bytes — how every
//! table in the crate points at a run of another one.

use std::ops::Range;

/// `(start, len)` index range over a flat arena. Compact — 8 bytes —
/// because measure-cache snapshots and grid hug slots store many of
/// these and we want to keep the per-entry footprint small.
///
/// [`Span::new`] is the constructor. `From` converts both ways against
/// `Range<u32>` and `Range<usize>`, for callers that already hold a
/// `start..end`. `range()` returns `Range<usize>` for slicing into
/// `Vec<T>`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Span {
    pub(crate) start: u32,
    pub(crate) len: u32,
}

impl Span {
    /// A span of `len` entries starting at `start`.
    ///
    /// `const` and public because the baked icon format is a flat blob with
    /// spans beside it: a generated set writes those spans into a `const` that
    /// this crate then reads — see [`IconDef::svg`](crate::IconDef::svg).
    #[inline]
    pub const fn new(start: u32, len: u32) -> Self {
        Self { start, len }
    }

    #[inline]
    pub(crate) const fn range(self) -> Range<usize> {
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
