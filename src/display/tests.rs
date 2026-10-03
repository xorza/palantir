use super::*;

/// The product is what layout divides by, and the system factor alone
/// is what the window manager is told. At 2× and 125% a 1000-px
/// surface is 400 logical px to the UI and 500 to the platform.
/// `from_physical` leaves the user scale at `ONE`, so the two spaces
/// coincide until something sets it.
#[test]
fn the_two_spaces_divide_by_different_factors() {
    let surface = UVec2::new(1000, 600);
    for (label, display, product, logical, system_logical) in [
        (
            "2× at 125%",
            Display {
                user_scale: UserScale::new(1.25),
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

/// Every axis that moves the raster fails `raster_eq` and forces the
/// full repaint that follows from it — the surface, either scale, the
/// pixel snap. 1× at 200% shares 2× at 100%'s `scale_factor`, and so
/// its painted pixels, but not its window-manager space. The refresh
/// rate paces frames and paints nothing, so a monitor move that
/// changes only it compares equal.
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
                user_scale: UserScale::new(1.25),
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
                user_scale: UserScale::new(2.0),
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

/// A scale factor is usable from `EPS` up, finite: the boundary is
/// inclusive, and zero, negatives and non-finite values are not.
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

/// The windowed door keeps a usable platform scale and replaces any
/// other with 1 — an `f64` past `f32`'s range included, which narrows
/// to infinity.
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
