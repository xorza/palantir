//! A `(start, len)` range into a flat arena in eight bytes; how every table points at a run of another.

use std::ops::Range;

/// `(start, len)` index range over a flat arena, 8 bytes because measure-cache snapshots and grid hug slots store many. Public as the cursor in [`ImePreedit`](crate::ImePreedit) and the byte range of an icon's SVG in its set's blob.
///
/// Plain data: [`Span::new`] checks nothing. `From` converts both ways with `Range<u32>` and `Range<usize>`; [`Span::range`] returns `Range<usize>` for slicing.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Span {
    /// The first index.
    pub start: u32,
    /// How many indices it covers, from `start` on.
    pub len: u32,
}

impl Span {
    /// A span of `len` entries starting at `start`. `const` and public because the baked icon format is a flat blob with spans beside it, written by generated sets into a `const` (see [`IconDefinition::svg`](crate::IconDefinition::svg)).
    #[inline]
    pub const fn new(start: u32, len: u32) -> Self {
        Self { start, len }
    }

    /// `start..start + len`, to slice the arena or text it indexes.
    #[inline]
    pub const fn range(self) -> Range<usize> {
        let start = self.start as usize;
        start..start + self.len as usize
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
        s.range()
    }
}
