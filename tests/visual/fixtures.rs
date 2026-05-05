//! Visual fixtures — actual UI scenes rendered headlessly and
//! compared against committed golden PNGs. Grouped by topic; add new
//! fixtures by extending an existing module or creating a new one and
//! declaring it below.

// `damage` reaches into `palantir::support::internals::set_clear_on_damage`,
// which is gated on the `internals` feature. Integration tests don't get
// the lib's `cfg(test)`, so this module needs the feature explicitly:
// `cargo test --features internals`.
#[cfg(feature = "internals")]
mod damage;
mod hidpi;
mod layout;
mod scroll;
mod text;
mod widgets;

use palantir::Color;

/// Default scene background — matches `helloworld.rs` so fixtures look
/// roughly like the real shell at a glance.
pub(crate) const DARK_BG: Color = Color::rgb(0.08, 0.08, 0.10);
