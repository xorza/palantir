//! The public text-geometry surface: the answers half of
//! [`TextRun`](crate::text::run::TextRun).
//!
//! Nothing shaped escapes `src/text/`: [`TextProbe`] answers in plain
//! geometry, and the cosmic-text buffers behind it stay private —
//! [`layout_probe`](crate::text::layout_probe) is where the lease over
//! one of them lives.

use crate::primitives::rect::Rect;
use crate::primitives::size::Size;
use crate::text::layout_probe::{Caret, TextLayoutProbe};
use std::ops::Range;

/// Geometry queries over one shaped run, minted by
/// [`Ui::probe_text`](crate::Ui::probe_text).
///
/// **A live probe holds the shaper's exclusive borrow**, which is why it
/// borrows the `Ui` mutably: two overlapping probes are then E0499 at
/// compile time rather than a `RefCell` panic in someone's running app.
/// Two *sequential* probes are fine — end the first with a block, or let
/// a temporary drop at the end of its statement.
///
/// One lifetime, though the layout behind it tracks the shaper borrow and
/// the run's text separately: nothing here hands back a borrow of either,
/// so collapsing them costs nothing and spares every caller a second
/// `'_`.
#[derive(Debug)]
pub struct TextProbe<'a> {
    inner: TextLayoutProbe<'a, 'a>,
}

impl<'a> TextProbe<'a> {
    pub(crate) fn new(inner: TextLayoutProbe<'a, 'a>) -> Self {
        Self { inner }
    }

    /// Extent of the shaped run; `Size::ZERO` for empty text.
    pub fn size(&self) -> Size {
        self.inner.size
    }

    /// Where the caret sits at `byte_offset`.
    pub fn caret_at(&self, byte_offset: usize) -> Caret {
        self.inner.cursor_xy(byte_offset)
    }

    /// The byte offset a point lands on, in run-local coordinates —
    /// click-to-caret. Clamped to the run, so a point outside it answers
    /// the nearest end rather than nothing.
    pub fn byte_at(&self, x: f32, y: f32) -> usize {
        self.inner.byte_at_xy(x, y)
    }

    /// Every rect covering `range`, one per visual line, in run-local
    /// coordinates.
    ///
    /// A callback rather than a returned collection: the rects are
    /// consumed immediately (painted, or unioned) and a caller that
    /// wants to retain them can push into a buffer it already owns, so
    /// nothing here allocates per frame.
    pub fn selection_rects(&self, range: Range<usize>, mut f: impl FnMut(Rect)) {
        self.inner.selection_rects(range, &mut f);
    }

    /// 64-bit hash of the run's text, as the shaping cache keys it —
    /// for a caller comparing "is this the same string as last frame?"
    /// without retaining a copy of it.
    pub fn text_hash(&self) -> u64 {
        self.inner.request.key.text_hash
    }
}

#[cfg(test)]
mod tests;
