//! [`FontScope`] — which faces a shaper's font database starts with.

use crate::text::cosmic;
use crate::text::font_family::FontFamily;
use cosmic_text::{FontSystem, fontdb};
use std::sync::Arc;

/// Bundled faces shipped with the crate: Inter is the default UI /
/// proportional family, JetBrains Mono the monospace, each as an upright
/// and an italic variable-weight (`wght`) file. Regular and Bold come
/// from one file per style, instantiated on the `wght` axis. All four are
/// OFL 1.1.
const BUNDLED: [&[u8]; 4] = [
    include_bytes!("../../assets/fonts/Inter-VariableFont_opsz,wght.ttf"),
    include_bytes!("../../assets/fonts/Inter-Italic-VariableFont_opsz,wght.ttf"),
    include_bytes!("../../assets/fonts/JetBrainsMono[wght].ttf"),
    include_bytes!("../../assets/fonts/JetBrainsMono-Italic[wght].ttf"),
];

/// The locale [`FontScope::Bundled`] shapes in.
///
/// Fixed rather than asked of the machine, which is the whole point of
/// the bundled scope: a test that measures a width has to get the same
/// number on every host, and the locale steers script fallback.
const BUNDLED_LOCALE: &str = "en-US";

/// Whether a shaper sees the machine's installed fonts.
///
/// A policy, not a given. The scan is the one startup cost that scales
/// with what the *user* has installed — 14.8 ms for 774 faces here, and
/// fontdb reports ~860 ms on a cold disk cache — so who pays it is a
/// decision each host makes rather than a constructor's side effect.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FontScope {
    /// The four bundled faces and nothing else. Deterministic metrics on
    /// every machine, and about 6 µs to build. What
    /// [`TextShaper::new`](crate::TextShaper::new) uses, so a test never
    /// pays for a font directory it did not put there.
    Bundled,
    /// The bundled faces plus every font the OS has installed, which act
    /// as glyph fallback for scripts the bundled faces do not cover. Text
    /// metrics are then *not* identical across machines. What a window
    /// wants, and what [`WinitHostConfig`](crate::WinitHostConfig)
    /// defaults to.
    System,
}

impl FontScope {
    /// Build the font database this scope names, then warm the match keys
    /// for the bundled families.
    ///
    /// The warm-up is why this is one call rather than a constructor plus
    /// a step the caller remembers: cosmic builds a `FontMatchKey` per
    /// face the first time a family/weight/style triple is shaped, which
    /// is O(faces) and lands on whichever frame first drew that face.
    /// Paying it here moves it to startup — and, under
    /// `FontScan`, onto another thread.
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
                // The generic families resolve to what is actually here,
                // rather than to the platform names cosmic picks and this
                // scope deliberately did not load.
                db.set_sans_serif_family(FontFamily::SANS.name());
                db.set_serif_family(FontFamily::SANS.name());
                db.set_monospace_family(FontFamily::MONO.name());
                FontSystem::new_with_locale_and_db(BUNDLED_LOCALE.to_owned(), db)
            }
        };
        // The bundled pair, which is what a stock theme names and what
        // every family with no face of its own resolves to.
        cosmic::warm_matches(&mut font_system, &[FontFamily::SANS, FontFamily::MONO]);
        font_system
    }
}

#[cfg(test)]
pub(crate) mod test_support {
    /// The bundled Inter as the crate actually ships it, so a load case
    /// registers those bytes rather than a second `include_bytes!` of the
    /// same 875 KB file.
    pub(crate) const INTER: &[u8] = super::BUNDLED[0];

    /// The bundled JetBrains Mono, beside [`INTER`] for the same reason.
    ///
    /// What a case needs two of these for: a family that resolves to the
    /// fallback and *then* to itself needs one face already registered to
    /// shape the fallback against, and a second one still missing. One
    /// file alone can only be absent or present.
    pub(crate) const MONO: &[u8] = super::BUNDLED[2];
}
