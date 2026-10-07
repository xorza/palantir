//! Visual fixtures: UI scenes rendered headlessly and compared against golden PNGs.

mod blit;
mod color;
mod corners;
mod damage;
mod damage_oracle;
mod expander;
mod fade;
mod format_change;
mod gpu_view;
mod gradient;
mod hidpi;
mod icon;
mod image;
mod layout;
mod occlusion;
mod scroll;
mod shadow;
mod shapes;
mod tabs;
mod text;
mod user_scale;
mod widgets;

use palantir::RgbaF32;

use crate::harness::FIXTURE_PALETTE;
use std::fmt;

/// The scene background: the suite palette's window colour, `Harness::clear`'s default. Set `RgbaF32::BLACK` there for harder contrast.
pub(crate) const DARK_BG: RgbaF32 = FIXTURE_PALETTE.window_background;

/// Tolerance for a probed channel against its 8-bit sRGB value: linear `f32` through an `f16` tint into an 8-bit target errs only by the final rounding, within one step.
pub(crate) const SRGB_ROUND_TRIP: u8 = 1;

/// Every channel of `got` is within `tol` of `want`, or panic naming `what`.
#[track_caller]
pub(crate) fn assert_px(got: [u8; 4], want: [u8; 4], tol: u8, what: impl fmt::Display) {
    let worst = got
        .iter()
        .zip(want)
        .map(|(g, w)| g.abs_diff(w))
        .max()
        .unwrap();
    assert!(
        worst <= tol,
        "{what}: got {got:?}, want {want:?} — a channel is {worst} steps off, {tol} allowed",
    );
}
