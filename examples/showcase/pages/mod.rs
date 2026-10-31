//! One module per nav entry. A page exposes the `build` the shell's page table calls and emits `support::Section`s into the vstack it is handed.

pub(crate) mod clip;
pub(crate) mod colors;
pub(crate) mod containers;
pub(crate) mod controls;
pub(crate) mod custom_widget;
pub(crate) mod dock;
pub(crate) mod focus;
pub(crate) mod frame_bench;
pub(crate) mod gpu_view;
pub(crate) mod gradients;
pub(crate) mod icons;
pub(crate) mod images;
pub(crate) mod materials;
pub(crate) mod motion;
pub(crate) mod overlays;
pub(crate) mod scroll;
pub(crate) mod shadows;
pub(crate) mod shapes;
pub(crate) mod sizing;
pub(crate) mod strokes;
pub(crate) mod tabs;
pub(crate) mod text;
pub(crate) mod text_input;
pub(crate) mod windows;
