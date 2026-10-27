//! The framework every widget in [`crate::widgets`] is built on: the [`Widget`](widget::Widget) node, [`Configure`](configure::Configure) setters, responses, and the overlay scope.

pub(crate) mod activation;
pub(crate) mod configurator;
pub(crate) mod configure;
pub(crate) mod overlay_response;
pub(crate) mod overlay_scope;
pub(crate) mod response;
pub(crate) mod value_response;
pub(crate) mod widget;
