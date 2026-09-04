//! Moving the surface between frames, and the physical/logical split.

use crate::primitives::size::Size;
use crate::ui::frame_report::FramePaint;
use crate::ui::harness::tests::support::{SURFACE, button};
use crate::ui::harness::*;

#[test]
fn resize_and_set_display_move_the_surface_between_frames() {
    // The harness owns the `Display`, so a frame never takes one — these
    // two are the whole surface-mutation surface. Both must land on the
    // `Ui` and must read as a display change, which is what forces the
    // full repaint asserted below.
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
    assert_eq!(harness.ui.display.physical, bigger);

    // A DPI move changes `physical` and `system_scale` together, leaving
    // `logical_rect` identical — `resize` alone cannot express it.
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
    assert_eq!(harness.ui.display, dpi_move);
    assert_eq!(
        harness.ui.display.logical_size().w,
        bigger.x as f32,
        "the logical surface is unchanged — only the raster is",
    );
}

#[test]
fn scale_makes_the_surface_physical_and_positions_logical() {
    // Rule 10. At dpr 2 a 200×120 physical surface is 100×60 logical,
    // and pointer positions are in the latter.
    let harness = UiHarness::new(SURFACE).scale(2.0);
    let display = harness.display;

    assert_eq!(display.physical, SURFACE);
    assert_eq!(display.scale_factor(), 2.0);
    assert_eq!(display.logical_size().w, 100.0);
    assert_eq!(display.logical_size().h, 60.0);
}

/// The user scale multiplies onto the dpr, so the two together divide the
/// surface once. At dpr 2 and 125% the 200×120 surface is 80×48 logical,
/// while the window manager still sees 100×60.
#[test]
fn user_scale_multiplies_onto_the_dpr() {
    let harness = UiHarness::new(SURFACE)
        .scale(2.0)
        .user_scale(UserScale::new(1.25));
    let display = harness.display;

    assert_eq!(display.scale_factor(), 2.5);
    assert_eq!(display.logical_size(), Size::new(80.0, 48.0));
    assert_eq!(display.system_logical_size(), Size::new(100.0, 60.0));
    assert_eq!(
        harness.ui.user_scale(),
        UserScale::new(1.25),
        "the harness writes both homes of the value",
    );
}

/// A user-scale move between frames must escalate to a full repaint, the
/// same way a DPI move does — it is the same rasterization change.
#[test]
fn a_user_scale_move_repaints_in_full() {
    let mut harness = UiHarness::new(SURFACE);
    harness.prime(2, button);
    assert_eq!(harness.frame(button).paint(), FramePaint::Skip);

    let zoomed = Display {
        user_scale: UserScale::new(1.5),
        ..harness.display
    };
    assert_eq!(
        harness.set_display(zoomed).frame(button).paint(),
        FramePaint::Full,
    );
    assert_eq!(harness.frame(button).paint(), FramePaint::Skip);
}
