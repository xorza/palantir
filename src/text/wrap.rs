use crate::primitives::num::F32Ext;

/// Canonical width used by layout-time shaping and direct widget probes.
#[inline]
pub(crate) fn canonical_wrap_width(width: f32) -> f32 {
    width.max(0.0).fast_round()
}

/// Text shaping and overflow policy.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum TextWrap {
    /// Single line shaped once at unbounded width. Its natural line width is
    /// also its minimum content width, so it deliberately overflows a narrower
    /// slot instead of truncating.
    #[default]
    SingleLine,
    /// Single line shaped at unbounded width with zero minimum content width.
    /// The owner clips and scrolls the complete run.
    Scroll,
    /// Single line hard-truncated to the committed width without a marker.
    Truncate,
    /// Single line truncated to the committed width with a trailing ellipsis.
    Ellipsis,
    /// Wrap at word boundaries, falling back to character boundaries when one
    /// word cannot fit.
    Wrap,
    /// Wrap only at word boundaries; words wider than the committed width
    /// overflow rather than breaking.
    WrapWithOverflow,
}
