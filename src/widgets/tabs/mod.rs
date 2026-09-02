//! The tab widgets: the chip row on its own, and the page view built
//! over it.
//!
//! [`TabStrip`](tab_strip::TabStrip) is the shared one — the dock
//! records the same widget for every pane it draws, so a strip in a
//! dialog and a strip on a docked pane are one control with one theme.
//! [`TabbedView`](tabbed_view::TabbedView) is a strip over a content
//! area bound to a page index, and is a peer of
//! [`DockView`](crate::DockView) rather than a step toward it.

pub(crate) mod tab_item;
pub(crate) mod tab_strip;
pub(crate) mod tabbed_view;

#[cfg(test)]
mod tests;
