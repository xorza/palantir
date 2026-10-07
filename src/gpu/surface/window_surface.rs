//! One window's swapchain: the surface, its configuration, and the frames it hands out.

use glam::UVec2;

use crate::common::tracy;
use crate::gpu::device::requested_gpu::Gpu;
use crate::gpu::surface::render_target::{RenderTarget, TargetFormat};
use crate::gpu::surface::surface_manager::{swapchain_mode, vsync_of};
use crate::window::vsync::Vsync;

/// A window's swapchain, from [`SurfaceManager::make_surface`](crate::gpu::surface::surface_manager::SurfaceManager::make_surface). Created unconfigured; the host configures on first paint.
#[derive(Debug)]
pub(crate) struct WindowSurface {
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
}

impl WindowSurface {
    pub(super) const fn new(
        surface: wgpu::Surface<'static>,
        config: wgpu::SurfaceConfiguration,
    ) -> Self {
        Self { surface, config }
    }

    pub(crate) const fn size(&self) -> UVec2 {
        UVec2::new(self.config.width, self.config.height)
    }

    /// Point the swapchain at `size` and answer whether that changed it; the next frame's target check gates the rebuild.
    pub(crate) fn resize(&mut self, size: UVec2) -> bool {
        if self.size() == size {
            return false;
        }
        self.config.width = size.x;
        self.config.height = size.y;
        true
    }

    pub(crate) fn format(&self) -> TargetFormat {
        TargetFormat::new(self.config.format)
    }

    pub(crate) const fn vsync(&self) -> Vsync {
        vsync_of(self.config.present_mode)
    }

    /// Point the swapchain config at `vsync` and answer whether that changed it; compared by present mode, so writing the same state back reconfigures nothing.
    pub(crate) fn set_vsync(&mut self, vsync: Vsync) -> bool {
        if self.vsync() == vsync {
            return false;
        }
        self.config.present_mode = swapchain_mode(vsync);
        true
    }

    /// Rebuild the swapchain against the current configuration; waits for device idle, so gate it on a target-key change.
    pub(crate) fn configure(&self, gpu: &Gpu) {
        self.surface.configure(&gpu.device, &self.config);
    }

    /// Take the next frame to render into; the tracy zone closes on the acquire, where a vsync-paced frame blocks.
    pub(crate) fn acquire(&self) -> Acquired {
        let acquired = {
            tracy::zone!("Surface::acquire");
            self.surface.get_current_texture()
        };
        match acquired {
            wgpu::CurrentSurfaceTexture::Success(frame) => Acquired::Ready(SurfaceFrame(frame)),
            // Dropping the frame lets the caller reconfigure: `configure` panics with `PreviousOutputExists` while an acquired texture lives.
            wgpu::CurrentSurfaceTexture::Suboptimal(frame) => {
                Acquired::Suboptimal(SurfaceFrame(frame))
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

#[derive(Debug)]
pub(crate) enum Acquired {
    Ready(SurfaceFrame),
    /// A frame that still presents from a stale swapchain: present or drop it, then rebuild.
    Suboptimal(SurfaceFrame),
    /// The swapchain is gone. Rebuild it and paint again.
    Outdated,
    /// The acquire timed out. Paint again.
    Timeout,
    /// The call was rejected; back off rather than build a draw list nothing accepts.
    Validation,
    /// The window is not visible. Nothing to paint.
    Occluded,
}

#[derive(Debug)]
pub(crate) struct SurfaceFrame(wgpu::SurfaceTexture);

impl SurfaceFrame {
    pub(crate) fn target(&self) -> RenderTarget<'_> {
        RenderTarget::new(&self.0.texture)
    }

    pub(crate) fn present(self, gpu: &Gpu) {
        gpu.queue.present(self.0);
    }
}
