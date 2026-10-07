//! [`FontScope`] — which faces a shaper's font database starts with.

use crate::text::cosmic;
use crate::text::font_family::FontFamily;
use cosmic_text::{FontSystem, fontdb};
use std::sync::Arc;

/// Bundled faces: Inter (UI/proportional) and JetBrains Mono, each upright and italic variable-weight (`wght`); all OFL 1.1.
const BUNDLED: [&[u8]; 4] = [
    include_bytes!("../../assets/fonts/Inter-VariableFont_opsz,wght.ttf"),
    include_bytes!("../../assets/fonts/Inter-Italic-VariableFont_opsz,wght.ttf"),
    include_bytes!("../../assets/fonts/JetBrainsMono[wght].ttf"),
    include_bytes!("../../assets/fonts/JetBrainsMono-Italic[wght].ttf"),
];

/// The locale [`FontScope::Bundled`] shapes in; fixed so widths are identical on every host (the locale steers script fallback).
const BUNDLED_LOCALE: &str = "en-US";

/// Whether a shaper sees the machine's installed fonts. The scan is the one startup cost that scales with the user's fonts (14.8 ms for 774 faces; fontdb reports ~860 ms cold), so each host decides who pays.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FontScope {
    /// The four bundled faces only: deterministic metrics, ~6 µs to build. What [`TextShaper::new`](crate::TextShaper::new) uses.
    Bundled,
    /// Bundled faces plus every installed font as glyph fallback; metrics differ across machines. What [`WinitHostBuilder`](crate::WinitHostBuilder) defaults to.
    System,
}

impl FontScope {
    /// Build the font database this scope names, then warm the match keys for the bundled families: cosmic builds a `FontMatchKey` per face on first use, O(faces), which this moves to startup (or the `FontScan` thread).
    pub(crate) fn build(self) -> FontSystem {
        let sources = BUNDLED
            .into_iter()
            .map(|bytes| fontdb::Source::Binary(Arc::new(bytes)));
        let mut font_system = match self {
            Self::System => FontSystem::new_with_fonts(sources),
            Self::Bundled => {
                let mut db = fontdb::Database::new();
                for source in sources {
                    db.load_font_source(source);
                }
                // Generic families resolve to what is loaded, not to platform names this scope did not load.
                db.set_sans_serif_family(FontFamily::SANS.name());
                db.set_serif_family(FontFamily::SANS.name());
                db.set_monospace_family(FontFamily::MONO.name());
                FontSystem::new_with_locale_and_db(BUNDLED_LOCALE.to_owned(), db)
            }
        };
        // The bundled pair: what a stock theme names and every faceless family resolves to.
        cosmic::warm_matches(&mut font_system, &[FontFamily::SANS, FontFamily::MONO]);
        font_system
    }
}

#[cfg(test)]
pub(crate) mod internals {
    /// The bundled Inter as shipped, so a load case registers those bytes rather than a second `include_bytes!`.
    pub(crate) const INTER: &[u8] = super::BUNDLED[0];

    /// The bundled JetBrains Mono, beside [`INTER`]: a family that resolves to the fallback and then to itself needs one registered face and one still missing.
    pub(crate) const MONO: &[u8] = super::BUNDLED[2];

    /// Noto Sans Hebrew for right-to-left cases; a test asset, so cases shape the same glyphs on any host.
    pub(crate) const HEBREW: &[u8] =
        include_bytes!("../../assets/fonts/test/NotoSansHebrew-Regular.ttf");

    /// Noto Sans Arabic, beside [`HEBREW`] for the same reason.
    pub(crate) const ARABIC: &[u8] =
        include_bytes!("../../assets/fonts/test/NotoSansArabic-Regular.ttf");
}
