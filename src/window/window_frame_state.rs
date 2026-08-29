//! Host-owned window facts copied into `Ui` for a frame.

use crate::window::window_placement::WindowPlacement;

/// Host-owned facts copied into `Ui` at the start of a window frame.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct WindowFrameState {
    pub(crate) close_requested: bool,
    pub(crate) placement: WindowPlacement,
}
