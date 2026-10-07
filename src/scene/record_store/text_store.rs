//! The record pass's text arena.

use crate::common::hash;
use crate::common::span::Span;
use crate::primitives::text::interned_str::InternedStr;
use crate::primitives::text::recorded_text::RecordedText;
use crate::primitives::text::text_epoch::TextEpoch;
use std::fmt;
use std::fmt::Write as _;

/// One window's record-pass text: an arena cleared every pass and stamped with a fresh [`TextEpoch`],
/// so a stale handle cannot be resolved. One `String`, no interior cell: writers go through `&mut`
/// [`RecordStore`]; no rotation or pool, as text is interned per frame per window (see [`InternedStr`]).
///
/// [`RecordStore`]: crate::scene::record_store::RecordStore
#[derive(Debug)]
pub(super) struct TextStore {
    bytes: String,
    epoch: TextEpoch,
}

impl Default for TextStore {
    fn default() -> Self {
        Self {
            bytes: String::new(),
            epoch: TextEpoch::reserve(),
        }
    }
}

impl TextStore {
    pub(super) fn bytes(&self) -> &str {
        &self.bytes
    }

    /// Readies the arena for a new pass, dropping old bytes and retiring their handles via a fresh epoch.
    pub(super) fn clear(&mut self) {
        self.bytes.clear();
        self.epoch = TextEpoch::reserve();
    }

    pub(super) fn intern_str(&mut self, text: &str) -> InternedStr {
        let start = self.bytes.len();
        self.bytes.push_str(text);
        InternedStr::new(Span::new(start as u32, text.len() as u32), self.epoch)
    }

    pub(super) fn intern_fmt(&mut self, args: fmt::Arguments<'_>) -> InternedStr {
        let start = self.bytes.len();
        self.bytes.write_fmt(args).unwrap();
        let end = self.bytes.len();
        InternedStr::new(Span::new(start as u32, (end - start) as u32), self.epoch)
    }

    /// Screens a handle from outside this pass. A foreign epoch is caller error (its bytes are gone), so it
    /// panics even in release; a `u64` compare is cheap.
    fn assert_current(&self, text: InternedStr) {
        assert!(
            text.epoch == self.epoch,
            "InternedStr outlived the record pass that minted it — intern text \
             once per frame, in the window recording it",
        );
    }

    pub(super) fn reuse(&self, text: InternedStr) -> InternedStr {
        self.assert_current(text);
        text
    }

    pub(super) fn text(&self, text: InternedStr) -> &str {
        self.assert_current(text);
        &self.bytes[text.span.range()]
    }

    pub(super) fn record(&self, text: InternedStr) -> RecordedText {
        self.assert_current(text);
        RecordedText::new(text.span, hash::hash_str(&self.bytes[text.span.range()]))
    }
}
