//! The borrowed record-pass text arena that spans resolve against.

use std::cell::Ref;

/// Borrow of the complete record-pass text arena. Recorded text spans resolve
/// against this value while its borrow guard keeps the arena immutable.
#[derive(Debug)]
pub(crate) struct InternedText<'a> {
    pub(crate) bytes: Ref<'a, str>,
}
