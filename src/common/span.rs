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
    /// # Panics
    ///
    /// Panics if the range runs backwards.
    #[inline]
    #[track_caller]
    fn from(r: Range<u32>) -> Self {
        assert!(r.start <= r.end, "a span's range runs forward, got {r:?}");
        Self {
            start: r.start,
            len: r.end - r.start,
        }
    }
}

impl From<Range<usize>> for Span {
    /// # Panics
    ///
    /// Panics if the range runs backwards or its end is past `u32::MAX`.
    #[inline]
    #[track_caller]
    fn from(r: Range<usize>) -> Self {
        assert!(r.start <= r.end, "a span's range runs forward, got {r:?}");
        let [start, end] =
            [r.start, r.end].map(|i| u32::try_from(i).expect("a span's range ends within u32"));
        Self {
            start,
            len: end - start,
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

#[cfg(test)]
mod tests {
    use crate::common::span::Span;
    use crate::internals::panic_probe;

    /// Both range forms convert exactly up to `u32::MAX`; a backward range or a `usize` end past
    /// it panics, where the casts wrapped or truncated.
    #[test]
    fn range_conversions_fit_or_panic() {
        assert_eq!(Span::from(3_u32..7), Span::new(3, 4));
        assert_eq!(Span::from(3_usize..7), Span::new(3, 4));
        assert_eq!(Span::from(0..u32::MAX as usize), Span::new(0, u32::MAX));
        assert_eq!(Span::from(5_u32..5), Span::new(5, 0));
        let backward = |start: u32, end: u32| start..end;
        panic_probe::assert_panics_with("runs forward", || Span::from(backward(7, 3)));
        panic_probe::assert_panics_with("runs forward", || {
            let r = backward(7, 3);
            Span::from(r.start as usize..r.end as usize)
        });
        panic_probe::assert_panics_with("ends within u32", || Span::from(0..u32::MAX as usize + 1));
    }
}
