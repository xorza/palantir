use crate::primitives::num::F32Ext;
use crate::primitives::size::Size;
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
    /// the bounded resolve outright; `CosmicMeasure::shape_truncated`
    /// asks it again for the paths that bypass `TextSystem` (the public
    /// text probe, buffer restore), where it decides whether the cut
    /// runs at all. They must agree, so they share this.
    ///
    /// A fitting single-line truncation shapes glyphs identical to the
    /// root — truncated shaping is halign-independent and single-line by
    /// construction. Never true for [`Self::Wrap`]: cosmic bakes per-line
    /// halign offsets into wrapped buffers. `size.w` is ceil'd and the
    /// canonical width is integral, so this comparison matches the
    /// truncating path's cut decision exactly.
    pub(super) fn resolves_to_unbounded(self, unbounded: &TextRoot, target_width_px: f32) -> bool {
        matches!(self, LineFit::Clip | LineFit::Ellipsis)
            && unbounded.single_line
            && unbounded.size.w <= canonical_wrap_width(target_width_px)
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

#[cfg(test)]
mod tests {
    use crate::layout::cache::quantize_available;
    use crate::primitives::size::Size;
    use crate::text::root::TextRoot;
    use crate::text::wrap;
    use crate::text::wrap::{LineFit, TextWrap};

    /// Every policy, in declaration order — so a new one has to be added
    /// here to compile, rather than quietly escaping the sweeps below.
    const ALL: [TextWrap; 6] = [
        TextWrap::SingleLine,
        TextWrap::Scroll,
        TextWrap::Truncate,
        TextWrap::Ellipsis,
        TextWrap::Wrap,
        TextWrap::WrapWithOverflow,
    ];

    /// The whole policy-to-fit mapping, plus the reachability it implies.
    ///
    /// Pinned because two things lean on it and neither would fail loudly.
    /// A policy that changed which fit it binds under would reshape every
    /// run beneath it against a different cache identity. And the
    /// shaper-level `TestShape` fixture takes a [`LineFit`] directly
    /// rather than a policy, which only stays honest while every fit is
    /// some policy's — a fit no policy yields would let a test describe a
    /// run layout cannot produce.
    #[test]
    fn every_line_fit_is_some_policys_and_only_the_two_unbounded_ones_have_none() {
        for (policy, expected) in [
            (TextWrap::SingleLine, None),
            (TextWrap::Scroll, None),
            (TextWrap::Truncate, Some(LineFit::Clip)),
            (TextWrap::Ellipsis, Some(LineFit::Ellipsis)),
            (TextWrap::Wrap, Some(LineFit::Wrap)),
            (TextWrap::WrapWithOverflow, Some(LineFit::Wrap)),
        ] {
            assert_eq!(policy.line_fit(), expected, "{policy:?}");
        }
        for fit in [LineFit::Wrap, LineFit::Clip, LineFit::Ellipsis] {
            assert!(
                ALL.iter().any(|policy| policy.line_fit() == Some(fit)),
                "{fit:?} is reachable from no TextWrap, so a fixture taking \
                 one directly can build a request layout never does",
            );
        }
        // Exactly two policies keep their unbounded shape; if that grew,
        // the `(width, fit)` gate would be letting more through than the
        // two documented on `line_fit`.
        assert_eq!(ALL.iter().filter(|p| p.line_fit().is_none()).count(), 2);
    }

    /// Unbounded root standing in for a shaped measurement — the only
    /// input the bounded-shaping decisions read.
    fn root(width_px: f32, single_line: bool, intrinsic_min: f32) -> TextRoot {
        TextRoot {
            size: Size::new(width_px, 16.0),
            intrinsic_min: Some(intrinsic_min),
            single_line,
        }
    }

    #[test]
    fn only_a_fitting_single_line_truncation_reuses_the_unbounded_root() {
        // A truncating fit whose root already fits shapes identical
        // glyphs, so the reshape and its cache entry are skipped. Wrap
        // never qualifies (cosmic bakes per-line halign into the buffer),
        // and neither does a root that already broke or overflows.
        for (fit, single_line, target_width_px, expected) in [
            (LineFit::Clip, true, 100.0, true),
            (LineFit::Ellipsis, true, 100.0, true),
            (LineFit::Wrap, true, 100.0, false),
            (LineFit::Clip, false, 100.0, false),
            (LineFit::Clip, true, 99.0, false),
            // The comparison runs on the canonical (whole-px) wrap grid,
            // so 99.6 rounds up to the root's 100 and fits; 99.4 does not.
            (LineFit::Clip, true, 99.6, true),
            (LineFit::Clip, true, 99.4, false),
        ] {
            assert_eq!(
                fit.resolves_to_unbounded(&root(100.0, single_line, 0.0), target_width_px),
                expected,
                "{fit:?}, single_line={single_line}, width={target_width_px}",
            );
        }
    }

    #[test]
    fn only_wrap_with_overflow_floors_the_shaping_width_at_its_widest_segment() {
        // 40 px committed against a 60 px unbreakable segment: every
        // policy but WrapWithOverflow shapes at the committed width and
        // lets the segment break.
        let narrow = root(200.0, false, 60.0);
        for policy in [
            TextWrap::SingleLine,
            TextWrap::Scroll,
            TextWrap::Truncate,
            TextWrap::Ellipsis,
            TextWrap::Wrap,
        ] {
            assert_eq!(policy.target_width(40.0, &narrow), 40.0, "{policy:?}");
        }
        assert_eq!(
            TextWrap::WrapWithOverflow.target_width(40.0, &narrow),
            60.0,
            "the widest segment overflows instead of breaking",
        );
        assert_ne!(
            TextWrap::WrapWithOverflow.target_width(40.0, &narrow),
            TextWrap::Wrap.target_width(40.0, &narrow),
        );
        // A committed width already past the floor is used verbatim, so
        // the policy only ever raises the target.
        assert_eq!(
            TextWrap::WrapWithOverflow.target_width(80.0, &narrow),
            80.0,
            "a width above the floor must pass through",
        );
    }

    #[test]
    fn wrap_target_matches_cache_grid() {
        assert_eq!(
            wrap::canonical_wrap_width(100.1),
            wrap::canonical_wrap_width(100.4),
        );
        assert_eq!(
            wrap::canonical_wrap_width(99.6),
            wrap::canonical_wrap_width(100.4),
        );
        assert_ne!(
            wrap::canonical_wrap_width(100.4),
            wrap::canonical_wrap_width(100.6),
        );
        for width in [0.0_f32, 99.6, 100.1, 100.4, 250.4] {
            let cache_width = quantize_available(Size::new(width, 0.0)).x;
            assert_eq!(
                wrap::canonical_wrap_width(width) as i32,
                cache_width,
                "width={width}",
            );
        }
        // The wrap width adds one rule on top of the shared grid: an
        // over-constrained layout can commit a negative width, which the
        // cache would assert on, so it clamps to zero here first.
        for width in [-0.4_f32, -1.0, -1e9] {
            assert_eq!(wrap::canonical_wrap_width(width), 0.0, "width={width}");
        }
    }
}
