//! One window's swapchain: the surface, its configuration, and the frames it hands out.

use glam::UVec2;
use std::num::NonZeroU32;

use crate::common::tracy;
use crate::gpu::device::gpu::Gpu;
use crate::gpu::surface::render_target::RenderTarget;
use crate::gpu::texel::TargetFormat;
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
    /// The largest 2D texture side of the device it presents through, which its extent stays
    /// inside, as a window may be larger than the device backs.
    max_extent: NonZeroU32,
}

/// Whether [`WindowSurface::prepare`] rebuilt the swapchain, whose new images hold nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[must_use]
pub(crate) enum Prepared {
    Kept,
    Rebuilt,
}

impl WindowSurface {
    /// `surface` under `config`, its extent held inside `max_extent`.
    pub(super) const fn new(
        surface: wgpu::Surface<'static>,
        mut config: wgpu::SurfaceConfiguration,
        max_extent: NonZeroU32,
    ) -> Self {
        let size = clamp_extent(max_extent, UVec2::new(config.width, config.height));
        config.width = size.x;
        config.height = size.y;
        Self {
            surface: Some(surface),
            config,
            applied: None,
            max_extent,
        }
    }

    pub(crate) const fn size(&self) -> UVec2 {
        UVec2::new(self.config.width, self.config.height)
    }

    /// Point the swapchain at `size`, held inside what the device backs, and answer whether that
    /// changed it; the next [`Self::prepare`] rebuilds it.
    pub(crate) fn resize(&mut self, size: UVec2) -> bool {
        let size = clamp_extent(self.max_extent, size);
        if self.size() == size {
            return false;
        }
        self.config.width = size.x;
        self.config.height = size.y;
        true
    }

    pub(crate) fn format(&self) -> TargetFormat {
        TargetFormat::held(self.config.format)
    }

    /// Whether pixels can be copied out of the swapchain's images, as a backdrop's capture is.
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
    ///
    /// A configure on a lost device builds nothing, and wgpu panics on an acquire from a surface
    /// no configure built (`NotConfigured`), so the build counts only on a device still healthy
    /// after it; [`Self::acquire`] refuses until one does.
    pub(crate) fn prepare(&mut self, gpu: &Gpu) -> Prepared {
        let Some(surface) = &self.surface else {
            return Prepared::Kept;
        };
        if self.applied.as_ref() == Some(&self.config) {
            return Prepared::Kept;
        }
        surface.configure(&gpu.device, &self.config);
        if gpu.is_lost() {
            self.applied = None;
            return Prepared::Kept;
        }
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
        if self.applied.is_none() {
            return Acquired::Unconfigured;
        }
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

/// A surface extent the device can back: at least one texel, at most `max_extent` a side, as a
/// swapchain past the device's 2D texture limit fails wgpu's validation.
const fn clamp_extent(max_extent: NonZeroU32, size: UVec2) -> UVec2 {
    let max = max_extent.get();
    UVec2::new(clamp_side(size.x, max), clamp_side(size.y, max))
}

const fn clamp_side(side: u32, max: u32) -> u32 {
    if side < 1 {
        1
    } else if side > max {
        max
    } else {
        side
    }
}

/// The swapchain policy `vsync` asks for. Both are automatic policies every
/// surface accepts, so either applies to a live swapchain directly.
pub(super) const fn swapchain_mode(vsync: Vsync) -> wgpu::PresentMode {
    match vsync {
        Vsync::On => wgpu::PresentMode::AutoVsync,
        Vsync::Off => wgpu::PresentMode::AutoNoVsync,
    }
}

/// Which [`Vsync`] state `mode` paces like. A configuration holds only the
/// two modes [`swapchain_mode`] writes; the match names every mode, so none
/// reads as either through a catch-all arm. Free to keep graphics-API types out
/// of `crate::window`.
const fn vsync_of(mode: wgpu::PresentMode) -> Vsync {
    match mode {
        wgpu::PresentMode::AutoVsync | wgpu::PresentMode::Fifo | wgpu::PresentMode::FifoRelaxed => {
            Vsync::On
        }
        wgpu::PresentMode::AutoNoVsync
        | wgpu::PresentMode::Immediate
        | wgpu::PresentMode::Mailbox => Vsync::Off,
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
    /// No configure built the swapchain, as one on a lost device does not: back off until the
    /// device's recovery builds it.
    Unconfigured,
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

#[cfg(test)]
mod tests {
    use crate::gpu::surface::window_surface::{clamp_extent, swapchain_mode, vsync_of};
    use crate::window::vsync::Vsync;
    use glam::UVec2;
    use std::num::NonZeroU32;

    /// Each side lands in 1 ..= the device's limit, apart.
    #[test]
    fn an_extent_holds_inside_the_device() {
        let max = NonZeroU32::new(4096).unwrap();
        for (size, want) in [
            (UVec2::new(0, u32::MAX), UVec2::new(1, 4096)),
            (UVec2::new(4097, 0), UVec2::new(4096, 1)),
            (UVec2::new(1, 4096), UVec2::new(1, 4096)),
            (UVec2::new(800, 600), UVec2::new(800, 600)),
        ] {
            assert_eq!(clamp_extent(max, size), want, "{size}");
        }
    }

    /// Every resolvable mode classifies and [`swapchain_mode`] inverts it, so a per-frame
    /// `Ui::vsync` write-back reconfigures nothing.
    #[test]
    fn every_present_mode_classifies_and_the_classification_round_trips() {
        for (mode, expected) in [
            (wgpu::PresentMode::AutoVsync, Vsync::On),
            (wgpu::PresentMode::Fifo, Vsync::On),
            (wgpu::PresentMode::FifoRelaxed, Vsync::On),
            (wgpu::PresentMode::AutoNoVsync, Vsync::Off),
            (wgpu::PresentMode::Immediate, Vsync::Off),
            (wgpu::PresentMode::Mailbox, Vsync::Off),
        ] {
            assert_eq!(vsync_of(mode), expected, "{mode:?}");
        }
        for vsync in [Vsync::On, Vsync::Off] {
            assert_eq!(vsync_of(swapchain_mode(vsync)), vsync, "{vsync:?}");
        }
    }

    #[test]
    fn both_vsync_states_map_to_automatic_policies() {
        assert_eq!(swapchain_mode(Vsync::On), wgpu::PresentMode::AutoVsync);
        assert_eq!(swapchain_mode(Vsync::Off), wgpu::PresentMode::AutoNoVsync);
        assert_eq!(Vsync::default(), Vsync::On, "vsync is on unless asked off");
    }
}
