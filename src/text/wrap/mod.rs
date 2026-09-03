//! Wrap policy: [`TextWrap`], the sizes each policy derives from an
//! unbounded root measurement, and the break rule those sizes are
//! measured against.
//!
//! Nothing here shapes or caches. Every layout consequence of a policy is a
//! pure function of a measurement layout already holds, or of the text
//! itself.

use crate::layout::types::align::HAlign;
use crate::primitives::num::F32Ext;
use crate::primitives::size::Size;
use crate::text::key::WrapBound;
use crate::text::root::TextRoot;

/// Canonical width used by width-bounded cache identity
/// ([`crate::text::key::WrapBound::new`]) and the fitting-truncate
/// check in `TextSystem::measure`.
///
/// Snapped with [`F32Ext::quantize_px`], the same grid the measure cache
/// keys `available_q` on — the two must agree or a cached subtree could be
/// blitted against a shape measured at another width. All this adds is the
/// clamp: an over-constrained layout can commit a negative width, which the
/// cache would assert on.
#[inline]
pub(super) fn canonical_wrap_width(width: f32) -> f32 {
    width.max(0.0).quantize_px() as f32
}

/// Byte offsets in `text` that open a new unbreakable segment: the
/// UAX #14 break opportunities, minus the terminal one at `text.len()`,
/// which ends the text rather than opening a segment.
///
/// **The one statement of where a line may break.** Both metrics measure
/// the wrap floor behind [`TextRoot::intrinsic_min`] over segments
/// delimited by these — the cosmic side by walking glyphs, the mono
/// metric by counting bytes — so neither can claim a segment the shaper
/// would happily break. It is also the source cosmic-text splits its own
/// shape words on (`cosmic-text/src/shape.rs`).
///
/// Whitespace is not trimmed here: UAX #14 places the opportunity
/// *after* a space, so a space ends its segment and hangs. What each
/// measurer drops off the end of a segment belongs to how it measures
/// ink, not to where the breaks are.
pub(super) fn break_offsets(text: &str) -> impl Iterator<Item = u32> + '_ {
    unicode_linebreak::linebreaks(text)
        .map(|(offset, _)| offset)
        .filter(|&offset| offset < text.len())
        .map(|offset| offset as u32)
}

/// Whether a shape pays for the segment scan behind
/// [`TextRoot::intrinsic_min`].
///
/// Deliberately *not* part of
/// [`TextShapeRequest`](crate::text::request::TextShapeRequest):
/// it selects which fields of the result get filled in, not which buffer
/// answers. Two shapes differing only in this must share one cache entry,
/// which is why the floor is memoized onto the entry instead of keyed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum WrapFloor {
    /// Skip the scan; the floor stays `None`.
    Skip,
    /// Scan it, and memoize the result onto the cache entry.
    Scan,
}

/// How a width-bounded text run handles overflow. Maps from the public
/// [`TextWrap`] via [`TextWrap::line_fit`] (`SingleLine`/`Scroll` stay on
/// the unbounded path); folded into the shape cache key by
/// [`TextSystem::measure`](crate::text::system::TextSystem::measure).
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum LineFit {
    /// Multi-line reflow at the target width.
    Wrap = 0,
    /// One line, hard-cut to the target width with no marker.
    Clip = 1,
    /// One line, cut to the target width with a trailing `…`.
    Ellipsis = 2,
}

impl LineFit {
    /// Whether resolving this fit at `target_width_px` reproduces the
    /// unbounded root, letting the caller skip the second shape and the
    /// bounded cache entry it would mint.
    ///
    /// Two callers, one question. `TextSystem::measure` asks it to skip
    /// the bounded resolve outright, so a key that reaches
    /// `CosmicMeasure::shape_truncated` has already answered `false`
    /// there — and the restore path replays such a key, at a quantized
    /// width that round-trips exactly. `shape_truncated` asks anyway,
    /// because the cut it would otherwise run is not a no-op on a run
    /// that fits: it reserves the ellipsis and drops a cluster the fit
    /// test would have kept. They must agree, so they share this.
    ///
    /// A fitting single-line truncation shapes glyphs identical to the
    /// root — truncated shaping is halign-independent and single-line by
    /// construction. Never true for [`Self::Wrap`]: cosmic bakes per-line
    /// halign offsets into wrapped buffers. `size.w` is ceil'd and the
    /// canonical width is integral, so this comparison matches the
    /// truncating path's cut decision exactly.
    ///
    /// `width_px` arrives **canonical**: `commit` quantizes once and
    /// hands the same number here and to `WrapBound::new`, and
    /// `shape_truncated`'s comes back off a key that was minted from one.
    /// Quantizing again here is how the fit test and the key it decides
    /// about could come to be asking about different widths.
    pub(super) fn resolves_to_unbounded(self, unbounded: &TextRoot, width_px: f32) -> bool {
        matches!(self, LineFit::Clip | LineFit::Ellipsis)
            && unbounded.single_line
            && unbounded.size.w <= width_px
    }
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

/// Every layout consequence of a wrap policy is a pure function of the
/// unbounded root measurement (and, for `TextWrap::content_size`, the
/// resolved one) — no cache or shaping access. `TextSystem::measure`
/// returns measurements; these methods derive the sizes layout consumes.
impl TextWrap {
    /// Width-bounded shaping mode, or `None` for the policies that always
    /// keep the unbounded shape (`SingleLine`, `Scroll`).
    pub(super) fn line_fit(self) -> Option<LineFit> {
        match self {
            TextWrap::SingleLine | TextWrap::Scroll => None,
            TextWrap::Truncate => Some(LineFit::Clip),
            TextWrap::Ellipsis => Some(LineFit::Ellipsis),
            TextWrap::Wrap | TextWrap::WrapWithOverflow => Some(LineFit::Wrap),
        }
    }

    /// Whether this policy reads [`TextRoot::intrinsic_min`], and so
    /// whether shaping has to pay for the segment scan that produces it.
    ///
    /// Only [`Self::WrapWithOverflow`] does. The scan is a UAX #14 pass
    /// over the run plus a binary search per glyph — 8x the cost of the
    /// rest of the measurement on a short label and 25x on a paragraph —
    /// so the other five policies opt out and the floor stays `None`.
    pub(super) fn floor_scan(self) -> WrapFloor {
        match self {
            TextWrap::WrapWithOverflow => WrapFloor::Scan,
            TextWrap::SingleLine
            | TextWrap::Scroll
            | TextWrap::Truncate
            | TextWrap::Ellipsis
            | TextWrap::Wrap => WrapFloor::Skip,
        }
    }

    /// Min-content demand, from the `unbounded` root measurement
    /// (`TextSystem::measure` with no available width) — not a bounded
    /// resolve, whose height already reflects wrapping.
    pub(crate) fn min_content(self, unbounded: &TextRoot) -> Size {
        match self {
            TextWrap::SingleLine => unbounded.size,
            // Scroll owns clipping and panning; truncating and wrapping
            // runs can shrink to nothing.
            TextWrap::Scroll | TextWrap::Truncate | TextWrap::Ellipsis | TextWrap::Wrap => {
                Size::new(0.0, unbounded.size.h)
            }
            TextWrap::WrapWithOverflow => Size::new(unbounded.wrap_floor(), unbounded.size.h),
        }
    }

    /// Max-content demand, from the `unbounded` root measurement.
    pub(crate) fn max_content(self, unbounded: &TextRoot) -> Size {
        match self {
            // Scroll's full run creates no width demand.
            TextWrap::Scroll => Size::new(0.0, unbounded.size.h),
            TextWrap::SingleLine
            | TextWrap::Truncate
            | TextWrap::Ellipsis
            | TextWrap::Wrap
            | TextWrap::WrapWithOverflow => unbounded.size,
        }
    }

    /// Width a width-bounded shape actually targets under this policy,
    /// given the committed `available_width_px`. Only
    /// [`Self::WrapWithOverflow`] departs from the committed width: it
    /// floors at the widest unbreakable segment so those segments
    /// overflow rather than break — the same floor
    /// [`Self::min_content`] demands.
    pub(super) fn target_width(self, available_width_px: f32, unbounded: &TextRoot) -> f32 {
        match self {
            TextWrap::WrapWithOverflow => available_width_px.max(unbounded.wrap_floor()),
            TextWrap::SingleLine
            | TextWrap::Scroll
            | TextWrap::Truncate
            | TextWrap::Ellipsis
            | TextWrap::Wrap => available_width_px,
        }
    }

    /// What a run bound to `available_width_px` actually shapes at.
    ///
    /// **The one implementation of the binding sequence.** Both shaping
    /// entry points — the public probe through `TextShaper::layout` and
    /// layout's own `TextSystem::measure` — have to run the same three
    /// steps in the same order, or a caret answers against a buffer
    /// wrapped at a width the paint never used. Written twice, they were
    /// kept in step by hand, and had already drifted.
    ///
    /// `root` is called only for the policies whose decision reads it:
    /// `WrapWithOverflow` raises a too-narrow width to the wrap floor
    /// (and is exactly the policy that asks for the floor scan), and a
    /// truncating fit asks whether the text already fits. A plain `Wrap`
    /// consults neither, so it binds without paying for a root shape —
    /// which is why this takes a thunk rather than a `&TextRoot`.
    pub(super) fn commit(
        self,
        available_width_px: f32,
        halign: HAlign,
        fit: LineFit,
        root: impl FnOnce() -> TextRoot,
    ) -> WrapCommit {
        // Canonicalized once, at the top: the fit test compares against
        // it and `WrapBound::new` keys on it, and a width quantized twice
        // is a width the two halves could disagree about.
        let available = canonical_wrap_width(available_width_px);
        let committed = if self.floor_scan() == WrapFloor::Scan || fit != LineFit::Wrap {
            let root = root();
            if fit.resolves_to_unbounded(&root, available) {
                return WrapCommit::Unbounded { size: root.size };
            }
            // Not canonical again: the wrap floor is a measured extent,
            // so `WrapBound::new` still quantizes what comes back.
            self.target_width(available, &root)
        } else {
            available
        };
        WrapCommit::Bound(WrapBound::new(committed, halign, fit))
    }

    /// Layout content contribution of a width-`resolved` extent.
    pub(crate) fn content_size(self, resolved: Size) -> Size {
        match self {
            TextWrap::Scroll => Size::new(0.0, resolved.h),
            TextWrap::SingleLine
            | TextWrap::Truncate
            | TextWrap::Ellipsis
            | TextWrap::Wrap
            | TextWrap::WrapWithOverflow => resolved,
        }
    }
}

/// What [`TextWrap::commit`] decided a width-bounded run shapes at.
#[derive(Clone, Copy, Debug)]
pub(super) enum WrapCommit {
    /// The root's own unbounded shape stands — a truncating fit whose
    /// text already fits. Binding would mint a second buffer nobody asks
    /// for, so the size travels out with the decision.
    Unbounded { size: Size },
    /// Resolve at this bound.
    Bound(WrapBound),
}

#[cfg(test)]
mod tests;
