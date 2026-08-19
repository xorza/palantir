//! Visual fixtures — actual UI scenes rendered headlessly and
//! compared against committed golden PNGs. Grouped by topic; add new
//! fixtures by extending an existing module or creating a new one and
//! declaring it below.

mod damage;
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
mod text;
mod widgets;

use palantir::Color;

/// Default scene background — a dark surrogate so fixtures look
/// roughly like a real shell at a glance. Not tied to any specific
/// demo; override per-fixture if a brighter contrast is needed.
pub(crate) const DARK_BG: Color = Color::rgb(0.08, 0.08, 0.10);

/// Pixel comparison shared by the exact-value fixtures: an sRGB round-trip
/// through the f16 tint and the render target moves a channel by at most one
/// step, so equality is "within two".
pub(crate) fn close(a: [u8; 4], b: [u8; 4]) -> bool {
    a.iter().zip(b).all(|(l, r)| l.abs_diff(r) <= 2)
}
