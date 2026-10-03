//! Visual fixtures — actual UI scenes rendered headlessly and
//! compared against stored golden PNGs. Grouped by topic; add new
//! fixtures by extending an existing module or creating a new one and
//! declaring it below.

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

/// The scene background most fixtures render on — the suite palette's own
/// window colour, so the ground matches the theme the widgets wear. It is
/// `Harness::clear`'s default, which every frame writes over
/// `Theme::window_clear`, so a fixture wanting harder contrast sets
/// `RgbaF32::BLACK` there instead.
pub(crate) const DARK_BG: RgbaF32 = FIXTURE_PALETTE.window_bg;

/// How far a probed channel may sit from the 8-bit value its fixture
/// derived: a colour written as linear `f32`, carried as an `f16` tint and
/// encoded into an 8-bit sRGB target lands within one step of its exact
/// sRGB encoding. `f16`'s 11-bit significand resolves every linear value
/// finer than the sRGB step that encodes it, so the only error left is
/// the final rounding to 8 bits — half a step each way, one step between
/// two values each rounded once.
pub(crate) const SRGB_ROUND_TRIP: u8 = 1;

/// Every channel of `got` is within `tol` of `want`, or a panic naming
/// `what`, both pixels and the worst channel.
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
