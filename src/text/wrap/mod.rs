//! Wrap policy: [`TextWrap`], the sizes each policy derives from an unbounded
//! root measurement, and the break rule those sizes are measured against.

use crate::primitives::geometry::size::Size;
use crate::primitives::layout::align::HAlign;
use crate::primitives::math::num::F32Px;
use crate::text::extent::TextExtent;
use crate::text::key::WrapBound;
use crate::text::root::TextRoot;

/// Byte offsets in `text` that open a new unbreakable segment: the UAX #14
/// break opportunities minus the terminal one at `text.len()`.
///
/// The one statement of where a line may break: both metrics measure the wrap
/// floor behind [`TextRoot::intrinsic_min`] over these segments. Whitespace is
/// not trimmed; the opportunity falls after a space, so it ends its segment.
pub(super) fn break_offsets(text: &str) -> impl Iterator<Item = u32> + '_ {
    unicode_linebreak::linebreaks(text)
        .map(|(offset, _)| offset)
        .filter(|&offset| offset < text.len())
        .map(|offset| offset as u32)
}

/// Whether a shape pays for the segment scan behind
/// [`TextRoot::intrinsic_min`].
///
/// Not part of [`TextShapeRequest`](crate::text::request::TextShapeRequest):
/// two shapes differing only in this share one cache entry, with the floor
/// memoized onto it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum WrapFloor {
    /// Skip the scan; the floor stays `None`.
    Skip,
    /// Scan it, and memoize the result onto the cache entry.
    Scan,
}

/// How a width-bounded text run handles overflow. Derived from [`TextWrap`]
/// by [`TextWrap::line_fit`]; part of the shape cache key.
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
    /// unbounded root, so the caller can skip the second shape.
    ///
    /// Shared by `TextSystem::measure` and `CosmicMeasure::shape_truncated`
    /// because the latter's cut is not a no-op on a run that fits: it reserves
    /// the ellipsis and may drop a cluster. Never true for [`Self::Wrap`],
    /// whose buffers bake in per-line halign offsets.
    ///
    /// `width_px` must be canonical (quantized once by `commit`); quantizing
    /// again could split the fit test from the key it decides about.
    pub(super) const fn resolves_to_unbounded(self, unbounded: &TextRoot, width_px: f32) -> bool {
        matches!(self, LineFit::Clip | LineFit::Ellipsis)
            && unbounded.single_line
            && unbounded.extent.size.w <= width_px
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
/// unbounded root measurement (and, for `content_size`, the resolved one).
impl TextWrap {
    /// Width-bounded shaping mode, or `None` for the policies that always
    /// keep the unbounded shape (`SingleLine`, `Scroll`).
    pub(super) const fn line_fit(self) -> Option<LineFit> {
        match self {
            TextWrap::SingleLine | TextWrap::Scroll => None,
            TextWrap::Truncate => Some(LineFit::Clip),
            TextWrap::Ellipsis => Some(LineFit::Ellipsis),
            TextWrap::Wrap | TextWrap::WrapWithOverflow => Some(LineFit::Wrap),
        }
    }

    /// Whether this policy reads [`TextRoot::intrinsic_min`], so shaping must
    /// pay for the segment scan. Only [`Self::WrapWithOverflow`] does; the
    /// scan costs 8x the rest of the measurement on a short label, 25x on a
    /// paragraph.
    pub(super) const fn floor_scan(self) -> WrapFloor {
        match self {
            TextWrap::WrapWithOverflow => WrapFloor::Scan,
            TextWrap::SingleLine
            | TextWrap::Scroll
            | TextWrap::Truncate
            | TextWrap::Ellipsis
            | TextWrap::Wrap => WrapFloor::Skip,
        }
    }

    /// Min-content demand from the `unbounded` root measurement, not a bounded
    /// resolve whose height already reflects wrapping.
    pub(crate) const fn min_content(self, unbounded: &TextRoot) -> Size {
        match self {
            TextWrap::SingleLine => unbounded.extent.size,
            // Scroll owns clipping; truncating and wrapping runs can shrink to nothing.
            TextWrap::Scroll | TextWrap::Truncate | TextWrap::Ellipsis | TextWrap::Wrap => {
                Size::new(0.0, unbounded.extent.size.h)
            }
            TextWrap::WrapWithOverflow => {
                Size::new(unbounded.wrap_floor(), unbounded.extent.size.h)
            }
        }
    }

    /// Max-content demand, from the `unbounded` root measurement.
    pub(crate) const fn max_content(self, unbounded: &TextRoot) -> Size {
        match self {
            TextWrap::Scroll => Size::new(0.0, unbounded.extent.size.h),
            TextWrap::SingleLine
            | TextWrap::Truncate
            | TextWrap::Ellipsis
            | TextWrap::Wrap
            | TextWrap::WrapWithOverflow => unbounded.extent.size,
        }
    }

    /// Width a width-bounded shape targets under `available_width_px`. Only
    /// [`Self::WrapWithOverflow`] departs from it, flooring at the widest
    /// unbreakable segment ([`Self::min_content`]'s floor).
    pub(super) const fn target_width(self, available_width_px: f32, unbounded: &TextRoot) -> f32 {
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
    /// The one implementation of the binding sequence, shared by the public
    /// `TextShaper::layout` probe and layout's `TextSystem::measure`, so a
    /// caret never answers against a buffer wrapped at a different width.
    ///
    /// `root` is a thunk because only `WrapWithOverflow` (floor) and truncating
    /// fits (already-fits test) read it; plain `Wrap` binds without a root
    /// shape.
    pub(super) fn commit(
        self,
        available_width_px: f32,
        halign: HAlign,
        fit: LineFit,
        root: impl FnOnce() -> TextRoot,
    ) -> WrapCommit {
        // Canonicalized once: the fit test compares against it and
        // `WrapBound::new` keys on it.
        let available = available_width_px.canonical_px();
        let committed = if self.floor_scan() == WrapFloor::Scan || fit != LineFit::Wrap {
            let root = root();
            if fit.resolves_to_unbounded(&root, available) {
                return WrapCommit::Unbounded {
                    extent: root.extent,
                };
            }
            // The wrap floor is a measured extent, so `WrapBound::new` still quantizes it.
            self.target_width(available, &root)
        } else {
            available
        };
        WrapCommit::Bound(WrapBound::new(committed, halign, fit))
    }

    /// Layout content contribution of a width-`resolved` extent.
    pub(crate) const fn content_size(self, resolved: Size) -> Size {
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
    /// The root's own unbounded shape stands (a truncating fit whose text
    /// already fits); the size travels out with the decision.
    Unbounded { extent: TextExtent },
    /// Resolve at this bound.
    Bound(WrapBound),
}

#[cfg(test)]
mod tests;
