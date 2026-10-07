//! The dock: a split tree of tabbed panes, the operations that rearrange it, and the widget that records it.
//!
//! The model ([`DockState`](dock_state::DockState), [`DockOperation`](dock_operation::DockOperation)) is pure data. The view ([`DockView`](dock_view::DockView)) emits operations and never learns what a pane contains; the application answers through [`DockTabs`](dock_tabs::DockTabs). Each pane's strip is the same [`TabStrip`](crate::TabStrip) a dialog records.

pub(crate) mod allowed_splits;
pub(crate) mod dock_node;
pub(crate) mod dock_operation;
pub(crate) mod dock_path;
pub(crate) mod dock_state;
pub(crate) mod dock_tab;
pub(crate) mod dock_tabs;
pub(crate) mod dock_view;
pub(crate) mod error;
pub(crate) mod pane_geometry;
pub(crate) mod split_side;
pub(crate) mod tab_drag;
pub(crate) mod tab_group;

#[cfg(test)]
mod tests;
