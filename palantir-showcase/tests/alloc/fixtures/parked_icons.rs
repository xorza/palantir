//! The one icon the audits draw, loaded once and held, as a scene holds its sets.

use palantir::{IconHandle, IconSet, IconTable, Ui};

/// One `16×16` box, so the SVG pipeline is what the rasterizer spends.
const ICON_SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16"><rect width="16" height="16" rx="3" fill="#fff"/></svg>"##;

/// A one-icon set, loaded at the first frame and *parked* across frames: an `IconSet` owns its
/// parses and rasters, so dropping it would unload and re-rasterize each frame.
#[derive(Debug, Default)]
pub(crate) struct ParkedIcons {
    held: Option<IconSet>,
}

impl ParkedIcons {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// The icon, from the set the first frame loaded.
    pub(crate) fn chip(&mut self, ui: &Ui) -> IconHandle {
        let icons = self.held.get_or_insert_with(|| {
            ui.load_icons(IconTable::from_svgs([("chip", ICON_SVG)]).unwrap())
        });
        icons.by_name("chip").expect("the one icon of the set")
    }
}
