//! The widget framework every widget in [`crate::widgets`] is built on: the
//! [`Widget`](widget::Widget) node, the [`Configure`](configure::Configure)
//! setters, the responses a widget hands back, the overlay scope, and the
//! per-state look a themed widget paints with.

pub(crate) mod configure;
pub(crate) mod overlay_response;
pub(crate) mod overlay_scope;
pub(crate) mod response;
pub(crate) mod select_response;
pub(crate) mod value_response;
pub(crate) mod widget;
pub(crate) mod widget_look;
