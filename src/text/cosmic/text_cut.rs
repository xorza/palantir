//! [`TextCut`]: which source bytes a truncating fit's buffer shows.

/// Which bytes of its source a truncating fit's buffer shows: a prefix, then, after its `…`, a
/// middle cut's kept end. Recorded when the cut is shaped, so a probe maps bytes through what
/// the shaper kept rather than inferring it by comparing texts, which a `…` in the source
/// fools.
#[derive(Clone, Copy, Debug)]
pub(crate) struct TextCut {
    /// The source's bytes `..prefix_end`, which the buffer shows from its start.
    pub(crate) prefix_end: usize,
    /// A middle cut's kept end.
    pub(crate) suffix: Option<CutSuffix>,
}

/// The source's bytes `src_start..src_start + len`, which the buffer shows from its byte
/// `buf_start` on.
#[derive(Clone, Copy, Debug)]
pub(crate) struct CutSuffix {
    pub(crate) buf_start: usize,
    pub(crate) src_start: usize,
    pub(crate) len: usize,
}
