//! Visual fixtures — actual UI scenes rendered headlessly and
//! compared against committed golden PNGs. Grouped by topic; add new
//! fixtures by extending an existing module or creating a new one and
//! declaring it below.

mod damage;
mod format_change;
mod gpu_view;
mod hidpi;
mod layout;
mod scroll;
mod text;
mod widgets;

use aperture::Color;

/// Default scene background — a dark surrogate so fixtures look
/// roughly like a real shell at a glance. Not tied to any specific
/// demo; override per-fixture if a brighter contrast is needed.
pub(crate) const DARK_BG: Color = Color::rgb(0.08, 0.08, 0.10);
