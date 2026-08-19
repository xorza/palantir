//! A record pass's handle to text it has already copied into its arena.

use crate::primitives::span::Span;
use crate::primitives::text_epoch::TextEpoch;

/// Text handle valid for **the record pass that minted it**, in the
/// window that minted it. A span into that pass's arena; construct one
/// with [`crate::Ui::intern`] or [`crate::Ui::fmt`], and lower it in the
/// same pass.
///
/// Lowering is zero-copy: the bytes are already where the record store
/// wants them, so recording is a span plus a hash.
///
/// **Intern per frame, per window.** Holding a handle past its pass —
/// into the next frame, into the second pass of a double-layout frame,
/// or into another window's — is a contract violation, and
/// [`crate::Ui`] panics rather than resolving it against whatever text
/// now occupies those offsets. Handles are [`Copy`] and carry no
/// ownership, so nothing keeps the bytes alive on their behalf.
///
/// Persistent application text belongs in its own `String`, passed to
/// widgets by reference; interning it each frame costs one `memcpy` into
/// the arena, which is what the borrowed path pays anyway.
#[derive(Clone, Copy, Debug)]
pub struct InternedStr {
    pub(crate) span: Span,
    pub(crate) epoch: TextEpoch,
}

impl InternedStr {
    pub(crate) fn new(span: Span, epoch: TextEpoch) -> Self {
        Self { span, epoch }
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.span.len == 0
    }
}
