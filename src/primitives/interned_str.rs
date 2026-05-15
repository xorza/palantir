use crate::primitives::span::Span;
use std::borrow::Cow;

/// Text input to widgets. Three carriers covering the common shapes a
/// caller hands a string to a widget:
///
/// - [`Borrowed`](Self::Borrowed) — pointer to a `&'static str` literal
///   (or any borrow outlasting the widget). Lowered into the active
///   tree's `text_bytes` arena via a memcpy at `add_shape` time.
/// - [`Owned`](Self::Owned) — a heap [`String`] (typical of
///   `format!()` results bound to a local). Same lowering as
///   `Borrowed`; the `String` is dropped at lowering.
/// - [`Interned`](Self::Interned) — bytes already live in the active
///   tree's `text_bytes` arena (produced by [`crate::Ui::fmt`]). The
///   `span` + `hash` were captured at write time; lowering is
///   **zero-copy** and the rollup hash is reused unchanged.
///
/// The third variant is the win: it lets callers format directly into
/// the destination buffer and skip both the per-call `String`
/// allocation and the lowering memcpy.
///
/// `'a` only constrains the `Borrowed` variant. Widget storage is
/// `InternedStr<'static>` — `Owned`/`Interned` are lifetime-free, and
/// `Borrowed` callers pass `&'static str` literals.
#[derive(Clone, Debug)]
pub enum InternedStr<'a> {
    Borrowed(&'a str),
    Owned(String),
    Interned { span: Span, hash: u64 },
}

impl InternedStr<'_> {
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

impl Default for InternedStr<'_> {
    #[inline]
    fn default() -> Self {
        Self::Borrowed("")
    }
}

impl<'a> From<&'a str> for InternedStr<'a> {
    #[inline]
    fn from(s: &'a str) -> Self {
        Self::Borrowed(s)
    }
}

impl From<String> for InternedStr<'static> {
    #[inline]
    fn from(s: String) -> Self {
        Self::Owned(s)
    }
}

impl<'a> From<Cow<'a, str>> for InternedStr<'a> {
    #[inline]
    fn from(c: Cow<'a, str>) -> Self {
        match c {
            Cow::Borrowed(s) => Self::Borrowed(s),
            Cow::Owned(s) => Self::Owned(s),
        }
    }
}
