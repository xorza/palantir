//! A span of source bytes in the active record store.

use crate::primitives::interned_text::InternedText;
use crate::primitives::span::Span;

/// Compact reference to source bytes in the active record store. Shared
/// by the two things that carry recorded text forward — the shape
/// record's [`RecordedText`](crate::primitives::recorded_text::RecordedText)
/// and the encoder's
/// [`ShapedTextRef`](crate::text::shaped_ref::ShapedTextRef) — so both
/// resolve their bytes through one definition.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct TextSource {
    pub(crate) span: Span,
}

impl TextSource {
    #[inline]
    pub(crate) fn resolve<'a>(self, interned_text: &'a InternedText<'_>) -> &'a str {
        &interned_text.bytes[self.span.range()]
    }
}
