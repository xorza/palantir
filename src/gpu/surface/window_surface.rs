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
    /// `None` once the platform lost it or the host [released](Self::release) it: a lost surface
    /// does not configure, so the host makes the window a new one ([`SurfaceManager::renew`](crate::gpu::surface::surface_manager::SurfaceManager::renew)),
    /// and the lost one goes first, as a platform may refuse a second surface on one window.
    surface: Option<wgpu::Surface<'static>>,
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
            surface: Some(surface),
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

    /// Whether pixels can be copied out of the swapchain's images, as a pane's capture is.
    pub(crate) const fn gives_copy(&self) -> bool {
        self.config.usage.contains(wgpu::TextureUsages::COPY_SRC)
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

    /// Whether the platform lost the surface, or the host let go of it, so the window needs a new
    /// one.
    pub(crate) const fn is_lost(&self) -> bool {
        self.surface.is_none()
    }

    /// Let go of the surface, which unconfigures on the device it was built for, ahead of a new
    /// one; it reads lost until then.
    pub(crate) fn release(&mut self) {
        self.surface = None;
        self.applied = None;
    }

    /// Rebuild the swapchain when the configuration it was built with is not the one wanted. A
    /// lost surface has nothing to rebuild: its acquire reports the loss.
    pub(crate) fn prepare(&mut self, gpu: &Gpu) -> Prepared {
        let Some(surface) = &self.surface else {
            return Prepared::Kept;
        };
        if self.applied.as_ref() == Some(&self.config) {
            return Prepared::Kept;
        }
        surface.configure(&gpu.device, &self.config);
        self.applied = Some(self.config.clone());
        Prepared::Rebuilt
    }

    /// Have the next [`Self::prepare`] rebuild the swapchain, as one the platform lost or
    /// reported stale.
    pub(crate) fn mark_stale(&mut self) {
        self.applied = None;
    }

    /// Take the next frame to render into; the tracy zone closes on the acquire, where a vsync-paced frame blocks.
    /// A surface the platform lost is dropped here, and every acquire until the host replaces it
    /// reports [`Acquired::Lost`].
    pub(crate) fn acquire(&mut self) -> Acquired {
        let Some(surface) = &self.surface else {
            return Acquired::Lost;
        };
        let acquired = {
            tracy::zone!("Surface::acquire");
            surface.get_current_texture()
        };
        match acquired {
            wgpu::CurrentSurfaceTexture::Success(frame) => Acquired::Ready(SurfaceFrame(frame)),
            // Dropping the frame lets the caller reconfigure: `configure` panics with `PreviousOutputExists` while an acquired texture lives.
            wgpu::CurrentSurfaceTexture::Suboptimal(frame) => {
                Acquired::Suboptimal(SurfaceFrame(frame))
            }
            wgpu::CurrentSurfaceTexture::Outdated => Acquired::Outdated,
            wgpu::CurrentSurfaceTexture::Lost => {
                self.release();
                Acquired::Lost
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
    /// The swapchain no longer matches the surface. Rebuild it and paint again.
    Outdated,
    /// The platform lost the surface, which no configure revives: the window needs a new one.
    Lost,
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
