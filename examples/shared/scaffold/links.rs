//! Which showcase page shows each observatory study's pieces alone: one table both examples'
//! headers read, the observatory's from the study side and the showcase's from the page side, so
//! the two lists cannot disagree. Each example's tests hold its own names in it to its own labels.

/// An observatory study, by its rail label, beside a showcase page, by its label.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Link {
    pub(crate) study: &'static str,
    pub(crate) page: &'static str,
}

const fn link(study: &'static str, page: &'static str) -> Link {
    Link { study, page }
}

pub(crate) const LINKS: &[Link] = &[
    link("Sky chart", "Scroll & zoom"),
    link("Sky chart", "GPU view"),
    link("Sky chart", "Gradients"),
    link("Sky chart", "Materials"),
    link("Sky chart", "Overlays"),
    link("Catalog", "Text input"),
    link("Catalog", "Scroll & zoom"),
    link("Catalog", "Overlays"),
    link("Catalog", "Focus & keyboard"),
    link("Session", "Controls"),
    link("Session", "Overlays"),
    link("Session", "Focus & keyboard"),
    link("Workspace", "Dock"),
    link("Workspace", "Tabs"),
    link("Workspace", "Overlays"),
    link("Workspace", "App & windows"),
    link("Appearance", "Colors"),
    link("Appearance", "Controls"),
];
