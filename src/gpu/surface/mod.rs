//! Where a frame lands: render targets, window swapchains and their manager, the retained
//! backbuffer and how a frame leaves it, readbacks, and scissors.

pub(crate) mod backbuffer;
pub(crate) mod capture;
pub(crate) mod damage_canvas;
#[cfg_attr(
    not(feature = "internals"),
    expect(
        unreachable_pub,
        reason = "`pub` for the `internals` re-export, which a build without it lacks"
    )
)]
pub(crate) mod owned_target;
pub(crate) mod render_target;
#[cfg(feature = "winit")]
pub(crate) mod surface_manager;
pub(crate) mod texel_readback;
#[cfg(feature = "winit")]
pub(crate) mod window_surface;
