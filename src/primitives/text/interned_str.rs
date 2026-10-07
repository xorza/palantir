//! A record pass's handle to text it has already copied into its arena.

use crate::common::span::Span;
use crate::primitives::text::text_epoch::TextEpoch;

/// Text handle valid for **the record pass and window that minted it**: a span into that pass's arena, made by [`crate::Ui::intern`] or [`crate::Ui::fmt`] and lowered in the same pass. Lowering is zero-copy (a span plus a hash).
///
/// **Intern per frame, per window.** Holding a handle into the next frame, the second pass of a double-layout frame, or another window's panics in [`crate::Ui`] rather than resolving to whatever text now occupies those offsets. Handles are [`Copy`] and own nothing.
///
/// Persistent text belongs in its own `String` passed by reference; interning it each frame costs one `memcpy`, as the borrowed path does.
#[derive(Clone, Copy, Debug)]
pub struct InternedStr {
    pub(crate) span: Span,
    pub(crate) epoch: TextEpoch,
}

impl InternedStr {
    pub(crate) const fn new(span: Span, epoch: TextEpoch) -> Self {
        Self { span, epoch }
    }

    /// Whether the interned run has no bytes.
    #[inline]
    pub const fn is_empty(&self) -> bool {
        self.span.len == 0
    }
}
