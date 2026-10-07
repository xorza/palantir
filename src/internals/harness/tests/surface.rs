//! Moving the surface between frames, and the physical/logical split.

use crate::internals::harness::tests::support::{SURFACE, button};
use crate::internals::harness::*;
use crate::primitives::geometry::size::Size;
use crate::ui::frame_report::FramePaint;

#[test]
fn resize_and_set_display_move_the_surface_between_frames() {
    // These two are the whole surface-mutation surface; both must read as a display change.
    let mut harness = UiHarness::new(SURFACE);
    harness.prime(2, button);
    assert_eq!(harness.display.physical, SURFACE);
    assert_eq!(harness.frame(button).paint(), FramePaint::Skip);

    let bigger = UVec2::new(400, 300);
    assert_eq!(
        harness.resize(bigger).frame(button).paint(),
        FramePaint::Full
    );
    assert_eq!(harness.display.physical, bigger);
    assert_eq!(harness.ui.display().physical, bigger);

    // A DPI move changes `physical` and `system_scale` together, which `resize` can't express.
    let dpi_move = Display {
        physical: bigger * 2,
        system_scale: 2.0,
        ..harness.display
    };
    assert_eq!(harness.frame(button).paint(), FramePaint::Skip);
    assert_eq!(
        harness.set_display(dpi_move).frame(button).paint(),
        FramePaint::Full,
    );
    assert_eq!(harness.ui.display(), dpi_move);
    assert_eq!(
        harness.ui.display().logical_size().w,
        bigger.x as f32,
        "the logical surface is unchanged — only the raster is",
    );
}

#[test]
fn scale_makes_the_surface_physical_and_positions_logical() {
    let harness = UiHarness::new(SURFACE).scale(2.0);
    let display = harness.display;

    assert_eq!(display.physical, SURFACE);
    assert_eq!(display.scale_factor(), 2.0);
    assert_eq!(display.logical_size().w, 100.0);
    assert_eq!(display.logical_size().h, 60.0);
}

/// At dpr 2 and 125% the 200×120 surface is 80×48 logical; the window manager sees 100×60.
#[test]
fn user_scale_multiplies_onto_the_dpr() {
    let mut harness = UiHarness::new(SURFACE)
        .scale(2.0)
        .user_scale(UserScale::new(1.25).unwrap());
    let display = harness.ui.display();

    assert_eq!(display.scale_factor(), 2.5);
    assert_eq!(display.logical_size(), Size::new(80.0, 48.0));
    assert_eq!(display.system_logical_size(), Size::new(100.0, 60.0));

    harness.frame(|ui| ui.set_user_scale(UserScale::new(1.5).unwrap()));
    harness.frame(button);
    assert_eq!(harness.ui.display().scale_factor(), 3.0);

    let swapped = Display {
        user_scale: UserScale::new(2.0).unwrap(),
        ..harness.ui.display()
    };
    harness.set_display(swapped);
    assert_eq!(harness.ui.user_scale(), UserScale::new(2.0).unwrap());
    assert_eq!(harness.ui.display().scale_factor(), 4.0);
}

#[test]
fn a_user_scale_move_repaints_in_full() {
    let mut harness = UiHarness::new(SURFACE);
    harness.prime(2, button);
    assert_eq!(harness.frame(button).paint(), FramePaint::Skip);

    let zoomed = Display {
        user_scale: UserScale::new(1.5).unwrap(),
        ..harness.ui.display()
    };
    assert_eq!(
        harness.set_display(zoomed).frame(button).paint(),
        FramePaint::Full,
    );
    assert_eq!(harness.frame(button).paint(), FramePaint::Skip);
}
