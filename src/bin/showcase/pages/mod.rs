//! One module per nav entry. A page exposes `build(&mut Ui)` and emits
//! `support::section`s into the vstack the shell hands it — no root
//! panel, no padding, no title of its own.

pub(crate) mod clip;
pub(crate) mod containers;
pub(crate) mod controls;
pub(crate) mod dialogs;
pub(crate) mod fixtures;
pub(crate) mod frame_bench;
pub(crate) mod gpu_view;
pub(crate) mod gradients;
pub(crate) mod icons;
pub(crate) mod images;
pub(crate) mod motion;
pub(crate) mod overlays;
pub(crate) mod pan_zoom;
pub(crate) mod scroll;
pub(crate) mod shadows;
pub(crate) mod shapes;
pub(crate) mod sizing;
pub(crate) mod state;
pub(crate) mod strokes;
pub(crate) mod text;
pub(crate) mod text_edit;
