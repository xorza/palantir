use crate::primitives::num::F32Ext;
use crate::primitives::size::Size;
use crate::text::TextMeasurement;
use crate::text::key::LineFit;

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

/// Every layout consequence of a wrap policy is a pure function of the
/// unbounded root measurement (and, for [`TextWrap::content_size`], the
/// resolved one) — no cache or shaping access. `TextSystem::measure`
/// returns measurements; these methods derive the sizes layout consumes.
impl TextWrap {
    /// Width-bounded shaping mode, or `None` for the policies that always
    /// keep the unbounded shape (`SingleLine`, `Scroll`).
    pub(crate) fn line_fit(self) -> Option<LineFit> {
        match self {
            TextWrap::SingleLine | TextWrap::Scroll => None,
            TextWrap::Truncate => Some(LineFit::Clip),
            TextWrap::Ellipsis => Some(LineFit::Ellipsis),
            TextWrap::Wrap | TextWrap::WrapWithOverflow => Some(LineFit::Wrap),
        }
    }

    /// Min-content demand, from the `unbounded` root measurement
    /// (`TextSystem::measure` with no available width) — not a bounded
    /// resolve, whose height already reflects wrapping.
    pub(crate) fn min_content(self, unbounded: &TextMeasurement) -> Size {
        match self {
            TextWrap::SingleLine => unbounded.size,
            // Scroll owns clipping and panning; truncating and wrapping
            // runs can shrink to nothing.
            TextWrap::Scroll | TextWrap::Truncate | TextWrap::Ellipsis | TextWrap::Wrap => {
                Size::new(0.0, unbounded.size.h)
            }
            TextWrap::WrapWithOverflow => Size::new(unbounded.intrinsic_min, unbounded.size.h),
        }
    }

    /// Max-content demand, from the `unbounded` root measurement.
    pub(crate) fn max_content(self, unbounded: &TextMeasurement) -> Size {
        match self {
            // Scroll's full run creates no width demand.
            TextWrap::Scroll => Size::new(0.0, unbounded.size.h),
            _ => unbounded.size,
        }
    }

    /// Layout content contribution of the `resolved` measurement.
    pub(crate) fn content_size(self, resolved: &TextMeasurement) -> Size {
        match self {
            TextWrap::Scroll => Size::new(0.0, resolved.size.h),
            _ => resolved.size,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::layout::cache::quantize_available;
    use crate::primitives::size::Size;
    use crate::text::wrap;

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
    }
}
