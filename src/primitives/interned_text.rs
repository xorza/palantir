//! The borrowed record-pass text arena that spans resolve against.

/// Borrow of the complete record-pass text arena. Recorded text spans
/// resolve against this value; the caller's `Ref<RecordPayloads>` is what
/// keeps the arena immutable for as long as this lives.
#[derive(Debug)]
pub(crate) struct InternedText<'a> {
    pub(crate) bytes: &'a str,
}
