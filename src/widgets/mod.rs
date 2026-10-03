//! The bundled widgets: one builder type per widget, each recording a
//! `Node` and its chrome into the frame.

#![expect(
    clippy::new_without_default,
    reason = "every widget constructor is `#[track_caller]` to mint the widget id, which a derived `Default` cannot capture, and a hand-written one would only rename `new()`"
)]

pub(crate) mod arrow;
pub(crate) mod axis_keys;
pub(crate) mod block;
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
pub(crate) mod context_menu;
pub(crate) mod dock;
pub(crate) mod drag_num;
pub(crate) mod drag_value;
pub(crate) mod expander;
pub(crate) mod gpu_view;
pub(crate) mod grid;
pub(crate) mod modal;
pub(crate) mod panel;
pub(crate) mod popup;
pub(crate) mod progress_bar;
pub(crate) mod radio;
pub(crate) mod scroll;
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

#[cfg(test)]
mod tests;
