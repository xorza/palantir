//! Host-owned window facts copied into `Ui` for a frame.

use glam::IVec2;

/// Host-owned facts copied into `Ui` at the start of a window frame.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct WindowFrameState {
    pub(crate) close_requested: bool,
    pub(crate) position: Option<IVec2>,
    pub(crate) maximized: bool,
}
