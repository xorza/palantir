//! Where a frame lands: render targets, window swapchains and their
//! manager, the retained backbuffer, the stencil attachment, and scissors.

pub(crate) mod backbuffer;
pub(crate) mod render_target;
pub(crate) mod stencil;
#[cfg(feature = "winit")]
pub(crate) mod surface_manager;
pub(crate) mod viewport;
#[cfg(feature = "winit")]
pub(crate) mod window_surface;
