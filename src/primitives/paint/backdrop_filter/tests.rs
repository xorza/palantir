use crate::primitives::paint::backdrop_filter::{BackdropFilter, Lens};
use crate::primitives::paint::chrome::Chrome;
use crate::primitives::paint::color::RgbaF32;
use crate::shape::Shape;
use crate::shape::paint::lowered_backdrop::LoweredBackdrop;

/// The looks set exactly their own fields, each `with_*` its own, and only `NONE` is a no-op.
#[test]
fn the_looks_and_setters_set_their_own_fields() {
    assert_eq!(
        BackdropFilter::blur(6.0),
        BackdropFilter {
            blur: 6.0,
            saturation: 1.0,
            noise: 0.0,
            max_luminance: None,
            lens: None,
        }
    );
    assert_eq!(
        BackdropFilter::frosted(12.0),
        BackdropFilter {
            blur: 12.0,
            saturation: 1.8,
            noise: 0.04,
            max_luminance: None,
            lens: None,
        }
    );
    let lens = Lens::default();
    assert_eq!(
        BackdropFilter::glass(lens),
        BackdropFilter {
            lens: Some(lens),
            ..BackdropFilter::NONE
        }
    );
    let set = BackdropFilter::NONE
        .with_blur(2.0)
        .with_saturation(0.5)
        .with_noise(0.1)
        .with_max_luminance(0.2)
        .with_lens(lens);
    assert_eq!(
        set,
        BackdropFilter {
            blur: 2.0,
            saturation: 0.5,
            noise: 0.1,
            max_luminance: Some(0.2),
            lens: Some(lens),
        }
    );
    assert_eq!(BackdropFilter::default(), BackdropFilter::NONE);
    assert!(BackdropFilter::NONE.is_noop());
    for filter in [
        BackdropFilter::blur(0.5),
        BackdropFilter::NONE.with_saturation(0.0),
        BackdropFilter::NONE.with_noise(0.01),
        BackdropFilter::NONE.with_max_luminance(1.0),
        BackdropFilter::glass(lens),
    ] {
        assert!(!filter.is_noop(), "{filter:?}");
    }
}

/// A rect and a chrome take the same checks: each case panics with its kind's rule.
#[test]
fn a_filter_out_of_its_kinds_panics_where_it_enters() {
    let lens = Lens::default();
    for (filter, rule) in [
        (BackdropFilter::blur(-1.0), "a length must"),
        (
            BackdropFilter::NONE.with_saturation(f32::NAN),
            "a factor must",
        ),
        (BackdropFilter::NONE.with_noise(-0.5), "a factor must"),
        (
            BackdropFilter::NONE.with_max_luminance(-0.1),
            "a factor must",
        ),
        (
            BackdropFilter::glass(Lens {
                thickness: f32::INFINITY,
                ..lens
            }),
            "a length must",
        ),
        (
            BackdropFilter::glass(Lens {
                refractive_index: 0.9,
                ..lens
            }),
            "a refractive index must",
        ),
    ] {
        for entry in ["rect", "chrome"] {
            let panic = std::panic::catch_unwind(|| match entry {
                "rect" => drop(Shape::rect(crate::Rect::ZERO).backdrop_filter(filter)),
                _ => Chrome::NONE.with_backdrop_filter(filter).validate(),
            })
            .expect_err(entry);
            let message = panic
                .downcast_ref::<String>()
                .map(String::as_str)
                .or_else(|| panic.downcast_ref::<&str>().copied())
                .unwrap_or_default();
            assert!(message.contains(rule), "{entry} {filter:?}: {message}");
        }
    }
}

/// A luminance ceiling lowers to the f16 at or below it, so a pane never shows brighter than
/// its author allowed: 0.3's nearest f16 is 0.300048828125 = 1229/4096, above it, so it lowers
/// one step down, to 0.2998046875 = 307/1024; 0.1's nearest, 0.0999755859375 = 819/8192, is
/// below it and stays; 0.5 is exact. A filter with no ceiling lowers to 65504, the lane's top,
/// and 1e6, which a tween may carry past the lane, saturates there.
#[test]
fn a_luminance_ceiling_rounds_down_where_it_lowers() {
    let lowered = |ceiling: Option<f32>| {
        LoweredBackdrop::from(BackdropFilter {
            max_luminance: ceiling,
            ..BackdropFilter::blur(1.0)
        })
        .max_luminance()
    };
    for (ceiling, want) in [
        (Some(0.3), 307.0 / 1024.0),
        (Some(0.1), 819.0 / 8192.0),
        (Some(0.5), 0.5),
        (Some(0.0), 0.0),
        (Some(1.0e6), 65504.0),
        (None, 65504.0),
    ] {
        assert_eq!(lowered(ceiling), want, "{ceiling:?}");
    }
}

/// A lens's dispersion and highlight clamp into `0..=1` where they lower to paint: 2 paints as 1,
/// −1 as 0, and NaN as 0.
#[test]
fn a_lens_fraction_clamps_where_it_paints() {
    let lowered = |dispersion, highlight| {
        LoweredBackdrop::from(BackdropFilter::glass(Lens {
            dispersion,
            highlight,
            ..Lens::default()
        }))
        .lens()
        .expect("a glass pane has a lens")
    };
    for (dispersion, highlight, want) in [
        (2.0, -1.0, (1.0, 0.0)),
        (f32::NAN, 0.5, (0.0, 0.5)),
        (0.25, 1.0, (0.25, 1.0)),
    ] {
        let lens = lowered(dispersion, highlight);
        assert_eq!(
            (lens.dispersion, lens.highlight),
            want,
            "{dispersion}, {highlight}"
        );
    }
}

/// `with_backdrop_filter` sets the filter alone, and a chrome whose only paint is a filter is not
/// a no-op: it makes a pane.
#[test]
fn a_chrome_with_only_a_filter_paints() {
    let base = Chrome::fill(RgbaF32::WHITE);
    let filter = BackdropFilter::frosted(8.0);
    let with = base.clone().with_backdrop_filter(filter);
    assert_eq!(
        with,
        Chrome {
            backdrop_filter: filter,
            ..base
        }
    );
    assert!(!Chrome::NONE.with_backdrop_filter(filter).is_noop());
}
