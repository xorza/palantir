//! Visual fixtures — actual UI scenes rendered headlessly and
//! compared against stored golden PNGs. Grouped by topic; add new
//! fixtures by extending an existing module or creating a new one and
//! declaring it below.

mod damage;
mod expander;
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
mod tabs;
mod text;
mod widgets;

use palantir::Color;

use crate::harness::FIXTURE_PALETTE;

/// The scene background most fixtures render on — the suite palette's own
/// window colour, so the ground matches the theme the widgets wear. It
/// arrives as `Harness::render`'s `clear` argument rather than from
/// `Theme::window_clear`, since a fixture wanting harder contrast passes
/// `Color::BLACK` instead.
pub(crate) const DARK_BG: Color = FIXTURE_PALETTE.terminal_bg;

/// Pixel comparison shared by the exact-value fixtures: an sRGB round-trip
/// through the f16 tint and the render target moves a channel by at most one
/// step, so equality is "within two".
pub(crate) fn close(a: [u8; 4], b: [u8; 4]) -> bool {
    a.iter().zip(b).all(|(l, r)| l.abs_diff(r) <= 2)
}
