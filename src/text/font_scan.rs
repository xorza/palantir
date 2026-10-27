//! [`FontScan`] — a font database being built on another thread.

use crate::text::cosmic::CosmicMeasure;
use crate::text::font_scope::FontScope;
use crate::text::shaper::TextShaper;
use cosmic_text::FontSystem;
use std::thread;
use std::thread::JoinHandle;

/// A [`FontScope::build`] running off the main thread, joined for the [`TextShaper`] it produces.
///
/// [`FontScope::System`] walks every OS font directory (14.8 ms for 774 faces warm, ~860 ms cold per fontdb), overlapping GPU init; the window waits only on a cold cache.
#[derive(Debug)]
pub(crate) struct FontScan {
    handle: JoinHandle<FontSystem>,
}

impl FontScan {
    pub(crate) fn spawn(scope: FontScope) -> Self {
        let handle = thread::Builder::new()
            .name("palantir-font-scan".to_owned())
            .spawn(move || scope.build())
            .expect("cannot spawn the font scan thread");
        Self { handle }
    }

    /// Block until the scan finishes. A scan-thread panic is re-raised, not swallowed into a bundled fallback, which would render non-Latin as tofu.
    pub(crate) fn join(self) -> TextShaper {
        let font_system = self.handle.join().expect("the font scan thread panicked");
        TextShaper::over(CosmicMeasure::over(font_system))
    }
}

#[cfg(test)]
mod tests {
    use crate::text::font_family::FontFamily;
    use crate::text::font_scan::FontScan;
    use crate::text::font_scope::FontScope;

    /// The scan thread hands back a usable shaper with bundled families resolvable (the contract `WinitRuntime::new` depends on): a database built on one thread survives the move to another. `Bundled`, so the case reads no disk and holds on every machine; what `System` adds is the directory walk, which is fontdb's.
    #[test]
    fn a_scanned_shaper_arrives_usable() {
        let shaper = FontScan::spawn(FontScope::Bundled).join();
        assert!(shaper.has_font(FontFamily::SANS));
        assert!(shaper.has_font(FontFamily::MONO));
        assert_eq!(shaper.font_epoch(), 0);
        assert_eq!(
            shaper.font_families(),
            [FontFamily::SANS, FontFamily::MONO],
            "the bundled pair, and nothing else"
        );
    }
}
