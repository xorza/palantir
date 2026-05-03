use std::ops::Range;

/// `(start, len)` index range over a flat arena. Compact — 8 bytes —
/// because measure-cache snapshots and grid hug slots store many of
/// these and we want to keep the per-entry footprint small. Use
/// `Range<u32>` (start..end) wherever start+end is a more natural
/// representation; this type is for the count-based form.
#[derive(Clone, Copy, Debug, Default)]
pub struct Span {
    pub start: u32,
    pub len: u32,
}

impl Span {
    pub const EMPTY: Self = Self { start: 0, len: 0 };

    #[inline]
    pub fn new(start: u32, len: u32) -> Self {
        Self { start, len }
    }

    #[inline]
    pub fn end(self) -> u32 {
        self.start + self.len
    }

    #[inline]
    pub fn range(self) -> Range<usize> {
        self.start as usize..self.end() as usize
    }

    #[inline]
    pub fn is_empty(self) -> bool {
        self.len == 0
    }
}
