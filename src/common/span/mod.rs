//! A `(start, len)` range into a flat arena in eight bytes; how every table points at a run of another.

use crate::common::span::error::SpanRangeError;
use std::ops::Range;

pub(crate) mod error;

/// `(start, len)` index range over a flat arena, 8 bytes because measure-cache snapshots and grid hug slots store many. Public as the cursor in [`ImePreedit`](crate::ImePreedit) and the byte range of an icon's SVG in its set's blob.
///
/// Plain data: [`Span::new`] checks nothing. `TryFrom` takes a `Range<u32>` or a `Range<usize>`, and [`Span::range`] gives the `Range<usize>` that slices.
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
    ///
    /// # Panics
    ///
    /// When the span ends past `usize::MAX`, as a span of plain data can on a 32-bit target,
    /// which no arena there holds.
    #[inline]
    pub const fn range(self) -> Range<usize> {
        let start = self.start as usize;
        match start.checked_add(self.len as usize) {
            Some(end) => start..end,
            None => panic!("a span ends within usize"),
        }
    }

    /// [`Self::range`] in `u32`, as a GPU draw counts instances and indices.
    ///
    /// # Panics
    ///
    /// When the span ends past `u32::MAX`, which no span of a real arena does.
    #[inline]
    pub(crate) const fn u32_range(self) -> Range<u32> {
        self.start
            ..self
                .start
                .checked_add(self.len)
                .expect("a span ends within u32")
    }
}

impl TryFrom<Range<u32>> for Span {
    type Error = SpanRangeError;

    fn try_from(r: Range<u32>) -> Result<Self, SpanRangeError> {
        let len = r.end.checked_sub(r.start).ok_or(SpanRangeError::Backward {
            start: r.start as usize,
            end: r.end as usize,
        })?;
        Ok(Self {
            start: r.start,
            len,
        })
    }
}

impl TryFrom<Range<usize>> for Span {
    type Error = SpanRangeError;

    fn try_from(r: Range<usize>) -> Result<Self, SpanRangeError> {
        let end = u32::try_from(r.end).map_err(|_| SpanRangeError::PastU32 { end: r.end })?;
        if r.start > r.end {
            return Err(SpanRangeError::Backward {
                start: r.start,
                end: r.end,
            });
        }
        Self::try_from(r.start as u32..end)
    }
}

#[cfg(test)]
mod tests {
    use crate::common::span::Span;
    use crate::common::span::error::SpanRangeError;
    use crate::internals::panic_probe;

    /// Both range forms convert exactly up to `u32::MAX`; a backward range or a `usize` end past
    /// it is refused, where the casts wrapped or truncated.
    #[test]
    fn range_conversions_fit_or_are_refused() {
        let backward = |start: u32, end: u32| start..end;
        let cases: [(Result<Span, SpanRangeError>, Result<Span, SpanRangeError>); 6] = [
            (Span::try_from(3_u32..7), Ok(Span::new(3, 4))),
            (Span::try_from(3_usize..7), Ok(Span::new(3, 4))),
            (
                Span::try_from(0..u32::MAX as usize),
                Ok(Span::new(0, u32::MAX)),
            ),
            (Span::try_from(5_u32..5), Ok(Span::new(5, 0))),
            (
                Span::try_from(backward(7, 3)),
                Err(SpanRangeError::Backward { start: 7, end: 3 }),
            ),
            (
                Span::try_from(0..u32::MAX as usize + 1),
                Err(SpanRangeError::PastU32 {
                    end: u32::MAX as usize + 1,
                }),
            ),
        ];
        for (got, want) in cases {
            assert_eq!(got, want);
        }
        let r = backward(7, 3);
        assert_eq!(
            Span::try_from(r.start as usize..r.end as usize),
            Err(SpanRangeError::Backward { start: 7, end: 3 })
        );
        assert_eq!(
            Span::new(u32::MAX - 2, 2).u32_range(),
            u32::MAX - 2..u32::MAX
        );
        panic_probe::assert_panics_with("ends within u32", || Span::new(u32::MAX, 1).u32_range());
    }
}
