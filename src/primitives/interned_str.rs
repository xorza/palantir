use crate::primitives::span::Span;
use smol_str::SmolStr;
use std::borrow::Cow;

/// Text input to widgets. Its representation is opaque so frame-local
/// arena handles can only be created by [`crate::Ui::fmt`] and
/// [`crate::Ui::intern`]. Two carriers are used internally:
///
/// - Owned — a [`SmolStr`]. `&'static str` literals
///   wrap via `SmolStr::new_static` (zero-copy fat pointer); strings
///   ≤ 23 bytes inline on the stack; longer strings sit behind an
///   `Arc<str>`. `.clone()` is always allocation-free.
/// - Interned — bytes already live in the active record pass's
///   `fmt_scratch` arena. The span, hash, and arena generation are
///   captured at write time; lowering is zero-copy and reuses the hash
///   after validating the generation.
///
/// Non-static `&str` callers route through `Ui::intern` (or
/// `Ui::fmt` for formatted output) to land in the `Interned` arm
/// without per-call allocation.
#[derive(Clone, Debug)]
pub struct InternedStr(pub(crate) InternedStrRepr);

#[derive(Clone, Debug)]
pub(crate) enum InternedStrRepr {
    Owned(SmolStr),
    Interned {
        span: Span,
        hash: u64,
        record_pass_generation: u64,
    },
}

impl InternedStr {
    pub(crate) fn frame_local(span: Span, hash: u64, record_pass_generation: u64) -> Self {
        Self(InternedStrRepr::Interned {
            span,
            hash,
            record_pass_generation,
        })
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        match &self.0 {
            InternedStrRepr::Owned(s) => s.is_empty(),
            InternedStrRepr::Interned { span, .. } => span.len == 0,
        }
    }

    /// Resolve to `&str`. Owned text carries the bytes inline (via
    /// `SmolStr::Deref`); frame-local text indexes into the record-pass
    /// arena passed in. The handle's arena generation was validated
    /// when its `ShapeRecord` was lowered.
    ///
    /// Crate-internal: only the frame pipeline holds the arena an
    /// `Interned` value indexes into, so this is not a public accessor.
    /// Consumers that keep their own text should store [`SmolStr`]
    /// (aperture re-exports it) and convert via `Into<InternedStr>`.
    #[inline]
    pub(crate) fn as_str<'a>(&'a self, text_bytes: &'a str) -> &'a str {
        match &self.0 {
            InternedStrRepr::Owned(s) => s,
            InternedStrRepr::Interned { span, .. } => {
                &text_bytes[span.start as usize..(span.start + span.len) as usize]
            }
        }
    }
}

impl Default for InternedStr {
    #[inline]
    fn default() -> Self {
        Self(InternedStrRepr::Owned(SmolStr::default()))
    }
}

impl From<&'static str> for InternedStr {
    #[inline]
    fn from(s: &'static str) -> Self {
        Self(InternedStrRepr::Owned(SmolStr::new_static(s)))
    }
}

impl From<String> for InternedStr {
    #[inline]
    fn from(s: String) -> Self {
        Self(InternedStrRepr::Owned(SmolStr::from(s)))
    }
}

impl From<SmolStr> for InternedStr {
    #[inline]
    fn from(s: SmolStr) -> Self {
        Self(InternedStrRepr::Owned(s))
    }
}

impl From<Cow<'static, str>> for InternedStr {
    #[inline]
    fn from(c: Cow<'static, str>) -> Self {
        match c {
            Cow::Borrowed(s) => Self(InternedStrRepr::Owned(SmolStr::new_static(s))),
            Cow::Owned(s) => Self(InternedStrRepr::Owned(SmolStr::from(s))),
        }
    }
}
