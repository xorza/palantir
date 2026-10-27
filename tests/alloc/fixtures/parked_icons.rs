//! The one icon the audits draw, loaded as an immediate-mode scene loads it.

use palantir::{IconHandle, IconSet, IconTable, Ui};
use std::rc::Rc;

/// One `16×16` box, so the SVG pipeline is what the rasterizer spends.
const ICON_SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16"><rect width="16" height="16" rx="3" fill="#fff"/></svg>"##;

/// A one-icon set, re-loaded inside each frame's scene as an immediate-mode caller writes it,
/// which pins that re-loading a live set is a refcount bump.
///
/// The set is *parked* across frames: an `IconSet` owns its parses and rasters, so dropping it
/// would unload and re-rasterize each frame (hence `#[must_use]` on `load_icons`).
#[derive(Debug)]
pub(crate) struct ParkedIcons {
    table: Rc<IconTable>,
    held: Option<IconSet>,
}

impl ParkedIcons {
    pub(crate) fn new() -> Self {
        Self {
            table: Rc::new(IconTable::from_svgs([("chip", ICON_SVG)]).unwrap()),
            held: None,
        }
    }

    /// The icon, from this frame's load. `insert` drops last frame's set *after* this frame's
    /// exists, so the shared owner never reaches zero.
    pub(crate) fn chip(&mut self, ui: &Ui) -> IconHandle {
        let icons = self.held.insert(ui.load_icons(Rc::clone(&self.table)));
        icons.by_name("chip").expect("the one icon of the set")
    }
}
