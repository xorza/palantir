use crate::primitives::span::Span;
use std::borrow::Cow;

/// Text input to widgets. Three carriers covering the common shapes a
/// caller hands a string to a widget:
///
/// - [`Borrowed`](Self::Borrowed) — pointer to a `&'static str` literal.
///   Stored as a fat pointer at lowering; no memcpy.
/// - [`Owned`](Self::Owned) — a heap [`String`] (typical of
///   `format!()` results bound to a local). Bytes stay in the
///   `String` allocation; dropped when the record drops next frame.
/// - [`Interned`](Self::Interned) — bytes already live in the active
///   frame's `fmt_scratch` arena (produced by [`crate::Ui::fmt`] /
///   [`crate::Ui::intern`]). The `span` + `hash` were captured at write
///   time; lowering is zero-copy and the rollup hash is reused unchanged.
///
/// Non-static `&str` callers route through `Ui::intern` (or
/// `Ui::fmt` for formatted output) to land in the `Interned` arm
/// without per-call allocation.
#[derive(Clone, Debug)]
pub enum InternedStr {
    Borrowed(&'static str),
    Owned(String),
    Interned { span: Span, hash: u64 },
}

impl InternedStr {
    #[inline]
    pub fn is_empty(&self) -> bool {
        match self {
            Self::Borrowed(s) => s.is_empty(),
            Self::Owned(s) => s.is_empty(),
            Self::Interned { span, .. } => span.len == 0,
        }
    }

    /// Resolve to `&str`. `Borrowed` / `Owned` carry the bytes inline;
    /// `Interned` indexes into the per-frame text arena passed in.
    /// Caller is responsible for passing the right arena — typically
    /// `&frame_arena.fmt_scratch` during the layout / readback pass.
    #[inline]
    pub fn as_str<'a>(&'a self, text_bytes: &'a str) -> &'a str {
        match self {
            Self::Borrowed(s) => s,
            Self::Owned(s) => s,
            Self::Interned { span, .. } => {
                &text_bytes[span.start as usize..(span.start + span.len) as usize]
            }
        }
    }
}

impl Default for InternedStr {
    #[inline]
    fn default() -> Self {
        Self::Borrowed("")
    }
}

impl From<&'static str> for InternedStr {
    #[inline]
    fn from(s: &'static str) -> Self {
        Self::Borrowed(s)
    }
}

impl From<String> for InternedStr {
    #[inline]
    fn from(s: String) -> Self {
        Self::Owned(s)
    }
}

impl From<Cow<'static, str>> for InternedStr {
    #[inline]
    fn from(c: Cow<'static, str>) -> Self {
        match c {
            Cow::Borrowed(s) => Self::Borrowed(s),
            Cow::Owned(s) => Self::Owned(s),
        }
    }
}
