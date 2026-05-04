use std::ops::Range;

/// `(start, len)` index range over a flat arena. Compact — 8 bytes —
/// because measure-cache snapshots and grid hug slots store many of
/// these and we want to keep the per-entry footprint small. Use
/// `Range<u32>` (start..end) wherever start+end is a more natural
/// representation; this type is for the count-based form.
#[derive(Clone, Copy, Debug, Default)]
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
