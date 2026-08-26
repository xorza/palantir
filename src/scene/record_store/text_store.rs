//! The record pass's text arena.

use crate::common::hash::hash_str;
use crate::primitives::interned_str::InternedStr;
use crate::primitives::recorded_text::RecordedText;
use crate::primitives::span::Span;
use crate::primitives::text_epoch::TextEpoch;
use std::fmt::Write as _;

/// One window's record-pass text. A single arena cleared at the start of
/// every pass, stamped with a fresh [`TextEpoch`] so a handle minted
/// against an earlier one cannot be resolved by mistake.
///
/// One `String` and no interior cell: [`RecordStore`] already owns the
/// arena behind a `RefCell`, so a second one here bought nothing but a
/// shared-outer / mutable-inner borrow pair on every intern.
///
/// [`RecordStore`]: crate::scene::record_store::RecordStore
///
/// There is no rotation and no pool. Both existed to keep an escaped
/// handle's bytes alive across passes, which meant a retained handle
/// pinned the whole frame's text and two retained passes minted a fresh
/// arena every frame. Requiring text to be interned per frame per window
/// removes the reason for either — see [`InternedStr`].
#[derive(Debug)]
pub(super) struct TextStore {
    bytes: String,
    epoch: TextEpoch,
}

impl Default for TextStore {
    fn default() -> Self {
        Self {
            bytes: String::new(),
            epoch: TextEpoch::next(),
        }
    }
}

impl TextStore {
    pub(super) fn bytes(&self) -> &str {
        &self.bytes
    }

    /// Ready the arena for a new record pass: drop the previous pass's
    /// bytes and take a fresh epoch, which is what retires every handle
    /// minted against them.
    ///
    /// Capacity is retained, so a steady scene re-interns the same text
    /// into the same allocation frame after frame.
    pub(super) fn clear(&mut self) {
        self.bytes.clear();
        self.epoch = TextEpoch::next();
    }

    pub(super) fn intern_str(&mut self, text: &str) -> InternedStr {
        let start = self.bytes.len();
        self.bytes.push_str(text);
        InternedStr::new(Span::new(start as u32, text.len() as u32), self.epoch)
    }

    pub(super) fn intern_fmt(&mut self, args: std::fmt::Arguments<'_>) -> InternedStr {
        let start = self.bytes.len();
        self.bytes.write_fmt(args).unwrap();
        let end = self.bytes.len();
        InternedStr::new(Span::new(start as u32, (end - start) as u32), self.epoch)
    }

    /// Lower a handle minted by this pass. Zero-copy — the bytes are
    /// already in place, so this is a bounds-checked slice and a hash.
    ///
    /// A foreign epoch is caller error, not bad data: the bytes the span
    /// addressed are gone, and resolving anyway would silently record
    /// whatever text now occupies those offsets. Debug-only because this
    /// runs per interned string per frame.
    pub(super) fn record(&self, text: InternedStr) -> RecordedText {
        debug_assert!(
            text.epoch == self.epoch,
            "InternedStr outlived the record pass that minted it — intern text \
             once per frame, in the window recording it",
        );
        RecordedText::new(text.span, hash_str(&self.bytes[text.span.range()]))
    }
}
