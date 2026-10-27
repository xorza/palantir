//! [`HostOptions`]: the settings both host builders take, so they read and apply them alike.

use crate::common::clipboard::Clipboard;
use crate::text::font_scope::FontScope;
use crate::text::shaper::TextShaper;

/// What a host seals at build beside its device and its own settings. Each builder writes its
/// defaults and documents them on its setters.
#[derive(Debug)]
pub(super) struct HostOptions {
    pub(super) fonts: FontScope,
    /// Wins over `fonts`, which then loads nothing.
    pub(super) shaper: Option<TextShaper>,
    pub(super) collect_gpu_stats: bool,
    pub(super) pixel_snap: bool,
    #[cfg(feature = "system-clipboard")]
    pub(super) system_clipboard: bool,
}

impl HostOptions {
    /// The shaper set, else the one `load` returns. A closure, so a set shaper skips the font
    /// load and the windowed host can join a scan it started earlier.
    pub(super) fn shaper_or_else(&self, load: impl FnOnce() -> TextShaper) -> TextShaper {
        self.shaper.clone().unwrap_or_else(load)
    }

    pub(super) fn clipboard(&self) -> Clipboard {
        #[cfg(feature = "system-clipboard")]
        if self.system_clipboard {
            return Clipboard::system_or_memory();
        }
        Clipboard::memory()
    }
}

#[cfg(test)]
mod tests {
    use crate::host::host_options::HostOptions;
    use crate::text::font_scope::FontScope;
    use crate::text::shaper::TextShaper;

    /// A set shaper wins over the scope and skips the load.
    #[test]
    fn a_set_shaper_skips_the_load() {
        let mut options = HostOptions {
            fonts: FontScope::Bundled,
            shaper: None,
            collect_gpu_stats: true,
            pixel_snap: false,
            #[cfg(feature = "system-clipboard")]
            system_clipboard: false,
        };
        let mut loads = 0;
        options.shaper_or_else(|| {
            loads += 1;
            TextShaper::test_mono()
        });
        assert_eq!(loads, 1, "no shaper set, so the scope loads");

        options.shaper = Some(TextShaper::test_mono());
        options.shaper_or_else(|| {
            loads += 1;
            TextShaper::test_mono()
        });
        assert_eq!(loads, 1, "the set shaper is used");
    }
}
