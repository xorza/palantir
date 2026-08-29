use crate::layout::cache::quantize_available;
use crate::primitives::size::Size;
use crate::text::root::TextRoot;
use crate::text::wrap;
use crate::text::wrap::{LineFit, TextWrap, canonical_wrap_width};

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
        // The comparison runs on the canonical (whole-px) wrap grid —
        // quantized by the caller, the way `commit` does it — so 99.6
        // rounds up to the root's 100 and fits; 99.4 does not.
        (LineFit::Clip, true, 99.6, true),
        (LineFit::Clip, true, 99.4, false),
    ] {
        assert_eq!(
            fit.resolves_to_unbounded(
                &root(100.0, single_line, 0.0),
                canonical_wrap_width(target_width_px),
            ),
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
