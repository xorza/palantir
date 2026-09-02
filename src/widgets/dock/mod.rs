//! The dock: a split tree of tabbed panes, the ops that rearrange it,
//! and the widget that records it.
//!
//! The model half ([`DockState`](dock_state::DockState) and its
//! [`DockOp`](dock_op::DockOp) vocabulary) is pure data with no `Ui` in
//! sight. The view half ([`DockView`](dock_view::DockView)) reads it and
//! emits ops, and never learns what a pane contains — the application
//! answers that through [`DockTabs`](dock_tabs::DockTabs).
//!
//! Every pane's strip is the same [`TabStrip`](crate::TabStrip) a
//! dialog would record, with the same theme and the same chip ids. The
//! dock does not reimplement tabs.

pub(crate) mod allowed_splits;
pub(crate) mod dock_node;
pub(crate) mod dock_op;
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
