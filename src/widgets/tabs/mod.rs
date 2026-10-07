//! The tab widgets: the chip row alone, and the page view over it.
//!
//! [`TabStrip`](tab_strip::TabStrip) is shared: the dock records it for every pane, so a dialog's strip and a docked pane's strip are one control with one theme. [`TabbedView`](tabbed_view::TabbedView) is a strip over a content area bound to a page index, a peer of [`DockView`](crate::DockView), not a step toward it.

pub(crate) mod tab_item;
pub(crate) mod tab_strip;
pub(crate) mod tabbed_view;

#[cfg(test)]
mod tests;
