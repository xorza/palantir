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
    link("sky chart", "pan & zoom"),
    link("sky chart", "gpu view"),
    link("sky chart", "gradients"),
    link("sky chart", "materials"),
    link("sky chart", "overlays"),
    link("catalog", "text input"),
    link("catalog", "scroll & split"),
    link("catalog", "overlays"),
    link("catalog", "focus & keyboard"),
    link("session", "controls"),
    link("session", "dialogs"),
    link("session", "focus & keyboard"),
    link("workspace", "dock"),
    link("workspace", "tabs"),
    link("workspace", "overlays"),
    link("workspace", "state & windows"),
    link("appearance", "colors"),
    link("appearance", "controls"),
];
