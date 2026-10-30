//! Where a frame lands: render targets, window swapchains and their
//! manager, the retained backbuffer, the stencil attachment, and scissors.

pub(crate) mod backbuffer;
pub(crate) mod capture;
pub(crate) mod damage_canvas;
#[cfg_attr(
    not(feature = "internals"),
    expect(
        unreachable_pub,
        reason = "`pub` for the `internals` re-export; a build without it reaches the type only \
                  through `HeadlessWindow`"
    )
)]
pub(crate) mod owned_target;
pub(crate) mod render_target;
pub(crate) mod stencil;
#[cfg(feature = "winit")]
pub(crate) mod surface_manager;
#[cfg(feature = "winit")]
pub(crate) mod surface_usages;
#[cfg(feature = "winit")]
pub(crate) mod window_surface;
