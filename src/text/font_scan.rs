//! [`FontScan`] — a font database being built on another thread.

use crate::text::cosmic::CosmicMeasure;
use crate::text::font_scope::FontScope;
use crate::text::shaper::TextShaper;
use cosmic_text::FontSystem;
use std::thread::JoinHandle;

/// A [`FontScope::build`] running off the main thread, joined for the
/// [`TextShaper`] it produces.
///
/// [`FontScope::System`] walks every font directory the OS has — 14.8 ms
/// for 774 faces on a warm disk cache here, and fontdb reports ~860 ms
/// cold. A window has one other startup cost of that order, GPU init, and
/// the two need nothing from each other. Started before the window is
/// created and joined when the shared host state is built, the scan costs
/// no wall time at all on a warm cache, and on a cold one the window
/// appears when it finishes — which is what happened before this existed.
#[derive(Debug)]
pub(crate) struct FontScan {
    handle: JoinHandle<FontSystem>,
}

impl FontScan {
    pub(crate) fn spawn(scope: FontScope) -> Self {
        let handle = std::thread::Builder::new()
            .name("palantir-font-scan".to_owned())
            .spawn(move || scope.build())
            .expect("cannot spawn the font scan thread");
        Self { handle }
    }

    /// Block until the scan finishes and wrap what it built.
    ///
    /// A panic on the scan thread is re-raised here rather than swallowed
    /// into a bundled fallback: a host that silently lost its system
    /// fonts renders every non-Latin script as tofu, and that is not a
    /// state to discover at runtime.
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

    /// The scan thread hands back a shaper the main thread can use, with
    /// the bundled families resolvable — the whole contract
    /// `WinitRuntime::new` depends on, and the one that would otherwise
    /// only be exercised by opening a window.
    ///
    /// `System` rather than `Bundled`, because the scan is the reason the
    /// thread exists: a `Bundled` join would pass without ever proving
    /// that a database built off-thread survives the move.
    #[test]
    fn a_scanned_shaper_arrives_usable() {
        let shaper = FontScan::spawn(FontScope::System).join();
        assert!(shaper.font_available(FontFamily::SANS));
        assert!(shaper.font_available(FontFamily::MONO));
        assert_eq!(shaper.font_epoch(), 0);
        assert!(
            shaper.font_families().len() > 2,
            "a system scan must find more than the bundled pair",
        );
    }
}
