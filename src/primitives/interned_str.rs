use crate::primitives::span::Span;
use smol_str::SmolStr;
use std::borrow::Cow;

/// Text input to widgets. Two carriers:
///
/// - [`Owned`](Self::Owned) — a [`SmolStr`]. `&'static str` literals
///   wrap via `SmolStr::new_static` (zero-copy fat pointer); strings
///   ≤ 23 bytes inline on the stack; longer strings sit behind an
///   `Arc<str>`. `.clone()` is always allocation-free.
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
    Owned(SmolStr),
    Interned { span: Span, hash: u64 },
}

impl InternedStr {
    #[inline]
    pub fn is_empty(&self) -> bool {
        match self {
            Self::Owned(s) => s.is_empty(),
            Self::Interned { span, .. } => span.len == 0,
        }
    }

    /// Resolve to `&str`. `Owned` carries the bytes inline (via
    /// `SmolStr::Deref`); `Interned` indexes into the per-frame text
    /// arena passed in. Caller is responsible for passing the right
    /// arena — typically `&frame_arena.fmt_scratch` during the layout
    /// / readback pass.
    #[inline]
    pub fn as_str<'a>(&'a self, text_bytes: &'a str) -> &'a str {
        match self {
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
        Self::Owned(SmolStr::default())
    }
}

impl From<&'static str> for InternedStr {
    #[inline]
    fn from(s: &'static str) -> Self {
        Self::Owned(SmolStr::new_static(s))
    }
}

impl From<String> for InternedStr {
    #[inline]
    fn from(s: String) -> Self {
        Self::Owned(SmolStr::from(s))
    }
}

impl From<SmolStr> for InternedStr {
    #[inline]
    fn from(s: SmolStr) -> Self {
        Self::Owned(s)
    }
}

impl From<Cow<'static, str>> for InternedStr {
    #[inline]
    fn from(c: Cow<'static, str>) -> Self {
        match c {
            Cow::Borrowed(s) => Self::Owned(SmolStr::new_static(s)),
            Cow::Owned(s) => Self::Owned(SmolStr::from(s)),
        }
    }
}
