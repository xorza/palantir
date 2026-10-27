//! One window's swapchain: the surface, its configuration, and the frames it hands out.

use glam::UVec2;

use crate::common::tracy;
use crate::gpu::device::gpu::Gpu;
use crate::gpu::surface::render_target::{RenderTarget, TargetFormat};
use crate::gpu::surface::surface_manager::{swapchain_mode, vsync_of};
use crate::window::vsync::Vsync;

/// A window's swapchain, from [`SurfaceManager::make_surface`](crate::gpu::surface::surface_manager::SurfaceManager::make_surface).
/// It holds the configuration the host wants beside the one the swapchain was last built with,
/// and [`Self::prepare`] rebuilds only when they differ: `surface.configure` waits for device
/// idle (wgpu #7447: 100 ms+ stalls), and Wayland resends identical configures.
#[derive(Debug)]
pub(crate) struct WindowSurface {
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    /// What the swapchain was built with; `None` before the first build and once it went stale.
    applied: Option<wgpu::SurfaceConfiguration>,
}

/// Whether [`WindowSurface::prepare`] rebuilt the swapchain, whose new images hold nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[must_use]
pub(crate) enum Prepared {
    Kept,
    Rebuilt,
}

impl WindowSurface {
    pub(super) const fn new(
        surface: wgpu::Surface<'static>,
        config: wgpu::SurfaceConfiguration,
    ) -> Self {
        Self {
            surface,
            config,
            applied: None,
        }
    }

    pub(crate) const fn size(&self) -> UVec2 {
        UVec2::new(self.config.width, self.config.height)
    }

    /// Point the swapchain at `size` and answer whether that changed it; the next
    /// [`Self::prepare`] rebuilds it.
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

    /// Point the swapchain at `vsync` and answer whether that changed it; compared by present
    /// mode, so writing the same state back rebuilds nothing.
    pub(crate) fn set_vsync(&mut self, vsync: Vsync) -> bool {
        if self.vsync() == vsync {
            return false;
        }
        self.config.present_mode = swapchain_mode(vsync);
        true
    }

    /// Rebuild the swapchain when the configuration it was built with is not the one wanted.
    pub(crate) fn prepare(&mut self, gpu: &Gpu) -> Prepared {
        if self.applied.as_ref() == Some(&self.config) {
            return Prepared::Kept;
        }
        self.surface.configure(&gpu.device, &self.config);
        self.applied = Some(self.config.clone());
        Prepared::Rebuilt
    }

    /// Have the next [`Self::prepare`] rebuild the swapchain, as one the platform lost or
    /// reported stale.
    pub(crate) fn mark_stale(&mut self) {
        self.applied = None;
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
