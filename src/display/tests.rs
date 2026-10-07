use super::*;

/// Layout divides by the product of the scales; the window manager gets the system factor alone: at 2x and 125% a 1000-px surface is 400 logical px to the UI, 500 to the platform.
#[test]
fn the_two_spaces_divide_by_different_factors() {
    let surface = UVec2::new(1000, 600);
    for (label, display, product, logical, system_logical) in [
        (
            "2× at 125%",
            Display {
                user_scale: UserScale::new(1.25).unwrap(),
                ..Display::from_physical(surface, 2.0)
            },
            2.5,
            Size::new(400.0, 240.0),
            Size::new(500.0, 300.0),
        ),
        (
            "2× with no user scale",
            Display::from_physical(surface, 2.0),
            2.0,
            Size::new(500.0, 300.0),
            Size::new(500.0, 300.0),
        ),
    ] {
        assert_eq!(display.scale_factor(), product, "{label}");
        assert_eq!(display.logical_size(), logical, "{label}");
        assert_eq!(display.system_logical_size(), system_logical, "{label}");
        assert_eq!(
            display.logical_rect(),
            Rect::new(0.0, 0.0, logical.w, logical.h),
            "{label}"
        );
    }
    assert_eq!(
        Display::from_physical(surface, 2.0).user_scale,
        UserScale::ONE
    );
}

/// Every axis that moves the raster fails `raster_eq` (surface, either scale, pixel snap); refresh rate paints nothing, so it compares equal.
#[test]
fn raster_eq_compares_every_raster_axis_and_only_those() {
    let base = Display::from_physical(UVec2::new(800, 600), 2.0);
    for (label, other, equal) in [
        ("same", base, true),
        (
            "physical",
            Display {
                physical: UVec2::new(801, 600),
                ..base
            },
            false,
        ),
        (
            "system scale",
            Display {
                system_scale: 1.0,
                ..base
            },
            false,
        ),
        (
            "user scale",
            Display {
                user_scale: UserScale::new(1.25).unwrap(),
                ..base
            },
            false,
        ),
        (
            "pixel snap",
            Display {
                pixel_snap: false,
                ..base
            },
            false,
        ),
        (
            "same product",
            Display {
                user_scale: UserScale::new(2.0).unwrap(),
                ..Display::from_physical(UVec2::new(800, 600), 1.0)
            },
            false,
        ),
        (
            "refresh",
            Display {
                refresh_millihertz: Some(60_000),
                ..base
            },
            true,
        ),
    ] {
        assert_eq!(base.raster_eq(&other), equal, "{label}");
        assert_eq!(other.raster_eq(&base), equal, "{label}: swapped");
    }
}

#[test]
fn scale_factor_is_valid_from_eps_up() {
    for (factor, valid) in [
        (1.0, true),
        (EPS, true),
        (EPS * 0.5, false),
        (0.0, false),
        (-1.0, false),
        (f32::NAN, false),
        (f32::INFINITY, false),
    ] {
        assert_eq!(scale_factor_is_valid(factor), valid, "{factor}");
    }
}

/// The windowed door replaces an unusable platform scale with 1.
#[cfg(feature = "winit")]
#[test]
fn sanitize_system_scale_keeps_a_usable_scale_or_falls_back_to_one() {
    for (reported, kept) in [
        (2.0_f64, 2.0_f32),
        (1.5, 1.5),
        (0.0, 1.0),
        (-2.0, 1.0),
        (f64::NAN, 1.0),
        (1e40, 1.0),
    ] {
        assert_eq!(sanitize_system_scale(reported), kept, "{reported}");
    }
}

#[test]
fn from_physical_checks_its_scale() {
    use crate::display::SCALE_RULE;
    use crate::internals::panic_probe;

    assert_eq!(Display::from_physical(UVec2::ONE, 2.0).system_scale, 2.0);
    assert_eq!(Display::from_physical(UVec2::ONE, 1e-4).system_scale, 1e-4);
    for bad in [0.0, 5e-5, -1.0, f32::NAN, f32::INFINITY] {
        panic_probe::assert_panics_with(SCALE_RULE, || Display::from_physical(UVec2::ONE, bad));
    }
}
