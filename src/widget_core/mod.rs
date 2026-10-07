//! The framework every widget in [`crate::widgets`] is built on: the [`Widget`](widget::Widget) node, [`Configure`](configure::Configure) setters, responses, the overlay scope, and the per-state look.

pub(crate) mod configure;
pub(crate) mod overlay_response;
pub(crate) mod overlay_scope;
pub(crate) mod response;
pub(crate) mod value_response;
pub(crate) mod widget;
pub(crate) mod widget_look;
