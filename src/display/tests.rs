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
                system_scale: SystemScale::ONE,
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

#[test]
fn from_physical_checks_its_scale() {
    use crate::display::system_scale::SystemScale;
    use crate::internals::panic_probe;

    assert_eq!(
        Display::from_physical(UVec2::ONE, 2.0).system_scale.get(),
        2.0
    );
    assert_eq!(
        Display::from_physical(UVec2::ONE, SystemScale::MIN)
            .system_scale
            .get(),
        SystemScale::MIN
    );
    for bad in [0.0, 1e-4, -1.0, f32::NAN, f32::INFINITY] {
        panic_probe::assert_panics_with("a system scale must be from", || {
            Display::from_physical(UVec2::ONE, bad)
        });
    }
}

/// A translation lands on whole physical px where the display snaps, half away from zero, and
/// stays as it is where it does not: at 2× 0.3 → 0.6 physical → 1 → 0.5 logical, and −0.3 →
/// −0.5; at 1.5× 0.4 → 0.6 → 1 → 2/3; at 1× 2.5 → 3. Unsnapped, 0.3 stays 0.3.
#[test]
fn a_translation_lands_on_whole_physical_px_where_the_display_snaps() {
    let surface = UVec2::new(800, 600);
    let at = |scale: f32| Display::from_physical(surface, scale);
    for (label, display, translation, landed) in [
        ("2×", at(2.0), Vec2::new(0.3, -0.3), Vec2::new(0.5, -0.5)),
        (
            "1.5×",
            at(1.5),
            Vec2::new(0.4, 0.0),
            Vec2::new(1.0 / 1.5, 0.0),
        ),
        ("1×", at(1.0), Vec2::new(2.5, -2.5), Vec2::new(3.0, -3.0)),
        (
            "unsnapped",
            Display {
                pixel_snap: false,
                ..at(2.0)
            },
            Vec2::new(0.3, -0.3),
            Vec2::new(0.3, -0.3),
        ),
    ] {
        assert_eq!(display.land_translation(translation), landed, "{label}");
    }
}
