//! One window's swapchain: the surface, the configuration it runs under, and
//! the frames it hands out.

use glam::UVec2;

use crate::common::tracy;
use crate::gpu::render_target::{RenderTarget, TargetFormat};
use crate::gpu::requested_gpu::Gpu;
use crate::gpu::surface_manager::{swapchain_mode, vsync_of};
use crate::window::vsync::Vsync;

/// A window's swapchain, produced by
/// [`SurfaceManager::make_surface`](crate::gpu::surface_manager::SurfaceManager::make_surface).
///
/// Created unconfigured. The host applies the configuration lazily on the
/// first paint, so opening a window costs no eager device reconfigure.
#[derive(Debug)]
pub(crate) struct WindowSurface {
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
}

impl WindowSurface {
    pub(super) fn new(surface: wgpu::Surface<'static>, config: wgpu::SurfaceConfiguration) -> Self {
        Self { surface, config }
    }

    pub(crate) fn size(&self) -> UVec2 {
        UVec2::new(self.config.width, self.config.height)
    }

    /// Point the swapchain at `size`, and answer whether that changed it.
    ///
    /// Writes the configuration only. The reconfigure itself waits for the
    /// next frame's target check, which is the one gate on rebuilding a
    /// swapchain.
    pub(crate) fn resize(&mut self, size: UVec2) -> bool {
        if self.size() == size {
            return false;
        }
        self.config.width = size.x;
        self.config.height = size.y;
        true
    }

    pub(crate) fn format(&self) -> TargetFormat {
        TargetFormat::from(self.config.format)
    }

    /// Which of [`Vsync`]'s two states this swapchain paces like.
    pub(crate) fn vsync(&self) -> Vsync {
        vsync_of(self.config.present_mode)
    }

    /// Point the swapchain config at `vsync`, and answer whether that changed
    /// it.
    ///
    /// The comparison runs against what the surface resolved the policy to,
    /// so a control that writes its own state back every frame reconfigures
    /// nothing.
    pub(crate) fn set_vsync(&mut self, vsync: Vsync) -> bool {
        if self.vsync() == vsync {
            return false;
        }
        self.config.present_mode = swapchain_mode(vsync);
        true
    }

    /// Rebuild the swapchain against the configuration as it now stands.
    ///
    /// Waits for the device to go idle and reallocates, so the caller gates it
    /// on a target-key change rather than calling it per event.
    pub(crate) fn configure(&self, gpu: &Gpu) {
        self.surface.configure(&gpu.device, &self.config);
    }

    /// Take the next frame to render into.
    ///
    /// The tracy zone closes on the acquire rather than spanning the submit
    /// that follows: on a vsync-paced present this call is where the frame
    /// blocks, and folding the submit into it hides which of the two cost the
    /// time.
    pub(crate) fn acquire(&self) -> Acquired {
        let acquired = {
            tracy::zone!("Surface::acquire");
            self.surface.get_current_texture()
        };
        match acquired {
            wgpu::CurrentSurfaceTexture::Success(frame) => Acquired::Ready(SurfaceFrame(frame)),
            // Dropping the frame here is what lets the caller reconfigure:
            // `configure` fails with `PreviousOutputExists` while an acquired
            // texture is still alive, and that failure is a panic, because
            // surface configuration reports through the device error sink.
            wgpu::CurrentSurfaceTexture::Suboptimal(frame) => {
                drop(frame);
                Acquired::Suboptimal
            }
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                Acquired::Outdated
            }
            wgpu::CurrentSurfaceTexture::Timeout => Acquired::Timeout,
            wgpu::CurrentSurfaceTexture::Validation => Acquired::Validation,
            wgpu::CurrentSurfaceTexture::Occluded => Acquired::Occluded,
        }
    }
}

/// What [`WindowSurface::acquire`] came back with.
#[derive(Debug)]
pub(crate) enum Acquired {
    /// A frame to render into, and then present.
    Ready(SurfaceFrame),
    /// The swapchain still works but no longer matches the surface. Rebuild
    /// it and paint again.
    Suboptimal,
    /// The swapchain is gone. Rebuild it and paint again.
    Outdated,
    /// The acquire timed out. Paint again.
    Timeout,
    /// The call itself was rejected. Painting again at once would build a
    /// full draw list for output nothing can accept, so the caller backs off.
    Validation,
    /// The window is not visible. Nothing to paint.
    Occluded,
}

/// A swapchain frame, alive until it is presented or dropped.
#[derive(Debug)]
pub(crate) struct SurfaceFrame(wgpu::SurfaceTexture);

impl SurfaceFrame {
    pub(crate) fn target(&self) -> RenderTarget<'_> {
        RenderTarget::from(&self.0.texture)
    }

    pub(crate) fn present(self, gpu: &Gpu) {
        gpu.queue.present(self.0);
    }
}
