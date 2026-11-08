//! Moving the surface between frames, and the physical/logical split.

use crate::display::user_scale::UserScale;
use crate::host::harness::tests::support::{self, SURFACE};
use crate::host::harness::*;
use crate::primitives::geometry::size::Size;
use crate::ui::frame_report::FramePaint;

#[test]
fn resize_and_set_system_scale_move_the_surface_between_frames() {
    // These two are the whole surface-mutation surface; both must read as a display change.
    let mut harness = Harness::mono(SURFACE);
    harness.prime(2, support::button);
    assert_eq!(harness.paint.target_key().physical, SURFACE);
    assert_eq!(harness.frame(support::button).paint(), FramePaint::Skip);

    let bigger = UVec2::new(400, 300);
    assert_eq!(
        harness.resize(bigger).frame(support::button).paint(),
        FramePaint::Full
    );
    assert_eq!(harness.paint.target_key().physical, bigger);
    assert_eq!(harness.ui().display().physical, bigger);

    // A DPI move changes `physical` and `system_scale` together.
    assert_eq!(harness.frame(support::button).paint(), FramePaint::Skip);
    assert_eq!(
        harness
            .resize(bigger * 2)
            .set_system_scale(SystemScale::new(2.0).unwrap())
            .frame(support::button)
            .paint(),
        FramePaint::Full,
    );
    assert_eq!(harness.ui().display().physical, bigger * 2);
    assert_eq!(
        harness.ui().display().system_scale,
        SystemScale::new(2.0).unwrap()
    );
    assert_eq!(
        harness.ui().display().logical_size().w,
        bigger.x as f32,
        "the logical surface is unchanged — only the raster is",
    );
}

/// The builder seeds the scheme, the motion, the transparency and the power the system reports,
/// as the setters set them; the first frame runs under them.
#[test]
fn the_builder_seeds_the_system_preferences() {
    let mut seeded = Harness::builder(SURFACE)
        .mono()
        .system_color_scheme(ColorScheme::Light)
        .system_motion_reduced(true)
        .system_transparency_reduced(true)
        .system_power_reduced(true)
        .build();
    assert_eq!(seeded.ui().system_color_scheme(), Some(ColorScheme::Light));
    assert!(seeded.ui().system_motion_reduced());
    assert!(seeded.ui().system_transparency_reduced());
    assert!(seeded.ui().system_power_reduced());
    seeded.frame(|_| {});
    assert!(seeded.ui().is_motion_reduced());
    assert!(seeded.ui().is_transparency_reduced());
    assert!(seeded.ui().is_power_reduced());
    let plain = Harness::builder(SURFACE).mono().build();
    assert_eq!(plain.ui().system_color_scheme(), None);
    assert!(!plain.ui().system_motion_reduced());
    assert!(!plain.ui().system_transparency_reduced());
    assert!(!plain.ui().system_power_reduced());
}

#[test]
fn scale_makes_the_surface_physical_and_positions_logical() {
    let harness = Harness::builder(SURFACE)
        .mono()
        .system_scale(SystemScale::new(2.0).unwrap())
        .build();
    let display = harness.ui().display();

    assert_eq!(display.physical, SURFACE);
    assert_eq!(display.scale_factor(), 2.0);
    assert_eq!(display.logical_size().w, 100.0);
    assert_eq!(display.logical_size().h, 60.0);
}

/// At dpr 2 and 125% the 200×120 surface is 80×48 logical; the window manager sees 100×60. A
/// user scale of 1.5 takes the factor to 3.
#[test]
fn user_scale_multiplies_onto_the_dpr() {
    let mut harness = Harness::builder(SURFACE)
        .mono()
        .system_scale(SystemScale::new(2.0).unwrap())
        .build();
    harness
        .ui_mut()
        .set_user_scale(UserScale::new(1.25).unwrap());
    harness.frame(support::button);
    let display = harness.ui().display();

    assert_eq!(display.scale_factor(), 2.5);
    assert_eq!(display.logical_size(), Size::new(80.0, 48.0));
    assert_eq!(display.system_logical_size(), Size::new(100.0, 60.0));

    harness.frame(|ui| ui.set_user_scale(UserScale::new(1.5).unwrap()));
    harness.frame(support::button);
    assert_eq!(harness.ui().display().scale_factor(), 3.0);
}

#[test]
fn a_user_scale_move_repaints_in_full() {
    let mut harness = Harness::mono(SURFACE);
    harness.prime(2, support::button);
    assert_eq!(harness.frame(support::button).paint(), FramePaint::Skip);

    harness
        .ui_mut()
        .set_user_scale(UserScale::new(1.5).unwrap());
    assert_eq!(harness.frame(support::button).paint(), FramePaint::Full);
    assert_eq!(harness.frame(support::button).paint(), FramePaint::Skip);
}
