//! The bundled widgets: one builder type per widget, each recording a
//! `Node` and its chrome into the frame.
//!
//! `clippy::new_without_default` is allowed module-wide. Every widget
//! constructor is `#[track_caller]` — the call site is what mints the
//! widget id — and `#[derive(Default)]` cannot capture one. A hand-written
//! `Default` could, but it would only be a second name for `new()` on a
//! builder that is constructed and consumed in one expression. The answer
//! is the same for every widget, so it is decided once here.
#![allow(clippy::new_without_default)]

pub(crate) mod axis_keys;
pub(crate) mod button;
pub(crate) mod checkbox;
pub(crate) mod checkerboard;
pub(crate) mod close_handle;
pub(crate) mod color_button;
pub(crate) mod color_field;
pub(crate) mod color_picker;
pub(crate) mod color_strip;
pub(crate) mod color_surface;
pub(crate) mod color_swatch;
pub(crate) mod combo_box;
pub(crate) mod configure;
pub(crate) mod context_menu;
pub(crate) mod dock;
pub(crate) mod drag_num;
pub(crate) mod drag_value;
pub(crate) mod expander;
pub(crate) mod frame;
pub(crate) mod gpu_view;
pub(crate) mod grid;
pub(crate) mod modal;
pub(crate) mod overlay_response;
mod overlay_scope;
pub(crate) mod panel;
pub(crate) mod popup;
pub(crate) mod progress_bar;
pub(crate) mod radio;
pub(crate) mod response;
pub(crate) mod scroll;
pub(crate) mod select_response;
pub(crate) mod separator;
pub(crate) mod slider;
pub(crate) mod spinner;
pub(crate) mod splitter;
pub(crate) mod switch;
pub(crate) mod tabs;
pub(crate) mod text;
pub(crate) mod text_edit;
pub(crate) mod theme;
pub(crate) mod toggle_chrome;
pub(crate) mod tooltip;
pub(crate) mod value_response;
pub(crate) mod widget;
