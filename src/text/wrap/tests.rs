use crate::layout::cache::MeasureCache;
use crate::primitives::geometry::size::Size;
use crate::primitives::math::num::F32Px;
use crate::text::extent::TextExtent;
use crate::text::root::TextRoot;
use crate::text::wrap::{LineFit, TextWrap};

/// Every policy, so a new one must be added here to compile.
const ALL: [TextWrap; 6] = [
    TextWrap::SingleLine,
    TextWrap::Scroll,
    TextWrap::Truncate,
    TextWrap::Ellipsis,
    TextWrap::Wrap,
    TextWrap::WrapWithOverflow,
];

/// The policy-to-fit mapping, pinned because cache identity and the
/// `TestShape` fixture (which takes a `LineFit` directly) depend on it.
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
    // Exactly two policies keep their unbounded shape.
    assert_eq!(ALL.iter().filter(|p| p.line_fit().is_none()).count(), 2);
}

/// Unbounded root standing in for a shaped measurement.
fn root(width_px: f32, single_line: bool, intrinsic_min: f32) -> TextRoot {
    TextRoot {
        extent: TextExtent::inked_within(Size::new(width_px, 16.0)),
        intrinsic_min: Some(intrinsic_min),
        single_line,
    }
}

#[test]
fn only_a_fitting_single_line_truncation_reuses_the_unbounded_root() {
    // A truncating fit whose root already fits skips the reshape. Wrap never
    // qualifies (cosmic bakes per-line halign into the buffer), nor does a
    // root that already broke or overflows.
    for (fit, single_line, target_width_px, expected) in [
        (LineFit::Clip, true, 100.0, true),
        (LineFit::Ellipsis, true, 100.0, true),
        (LineFit::Wrap, true, 100.0, false),
        (LineFit::Clip, false, 100.0, false),
        (LineFit::Clip, true, 99.0, false),
        // Compared on the whole-px wrap grid: 99.6 rounds up to the root's 100 and fits; 99.4 does not.
        (LineFit::Clip, true, 99.6, true),
        (LineFit::Clip, true, 99.4, false),
    ] {
        assert_eq!(
            fit.resolves_to_unbounded(
                &root(100.0, single_line, 0.0),
                target_width_px.canonical_px(),
            ),
            expected,
            "{fit:?}, single_line={single_line}, width={target_width_px}",
        );
    }
}

#[test]
fn only_wrap_with_overflow_floors_the_shaping_width_at_its_widest_segment() {
    // 40 px committed against a 60 px unbreakable segment: all but WrapWithOverflow shape at 40 and break it.
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
    // A committed width past the floor is used verbatim.
    assert_eq!(
        TextWrap::WrapWithOverflow.target_width(80.0, &narrow),
        80.0,
        "a width above the floor must pass through",
    );
}

#[test]
fn wrap_target_matches_cache_grid() {
    assert_eq!(100.1_f32.canonical_px(), 100.4_f32.canonical_px());
    assert_eq!(99.6_f32.canonical_px(), 100.4_f32.canonical_px());
    assert_ne!(100.4_f32.canonical_px(), 100.6_f32.canonical_px());
    for width in [0.0_f32, 99.6, 100.1, 100.4, 250.4] {
        let cache_width = MeasureCache::available_key(Size::new(width, 0.0)).x;
        assert_eq!(width.canonical_px() as i32, cache_width, "width={width}");
    }
    // A negative committed width (over-constrained layout) clamps to zero; the cache would assert on it.
    for width in [-0.4_f32, -1.0, -1e9] {
        assert_eq!(width.canonical_px(), 0.0, "width={width}");
    }
}
