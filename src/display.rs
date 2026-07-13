use crate::primitives::rect::Rect;
use crate::primitives::size::Size;
use glam::UVec2;

/// Display state for the current output: read by the renderer at
/// submit time, by hosts computing the logical surface rect for
/// layout, and by the repaint scheduler for frame pacing. Carries the
/// surface's physical pixel size, the DPR scale factor, the
/// snap-to-physical-pixel-edge flag, and the monitor's refresh rate.
///
/// The driving host rebuilds this each frame from the window's surface
/// config, scale factor, and monitor, then hands it to `WindowRenderer::frame`.
/// Changes that alter rasterized output are detected via [`Self::raster_eq`]
/// (physical size, scale, pixel snapping — a DPI-monitor move keeps
/// `logical_rect` constant yet must repaint); `refresh_millihertz` is
/// pacing-only and rides along without ever forcing a repaint.
///
/// Group exists so future rasterization knobs (sRGB correction, MSAA,
/// gamma) have a clear home.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Display {
    /// Physical surface size in pixels — same value the host hands
    /// to `wgpu::SurfaceConfiguration { width, height, .. }`.
    pub physical: UVec2,
    /// Logical→physical conversion factor (e.g. `2.0` on a 2× retina
    /// display). Must be ≥ `approx::EPS`; `Ui::frame` asserts.
    pub scale_factor: f32,
    /// Whether the composer snaps painted geometry edges (quad rects,
    /// shadow rects, image rects, text bounds, clip scissors) to
    /// integer physical pixels. Default `true` — sharper edges, no
    /// half-pixel blur. Mesh/curve/polyline vertices and corner radii
    /// are never snapped (would warp geometry). Damage scissors (fed
    /// to `wgpu::RenderPass::set_scissor_rect`, which only accepts
    /// `u32`) always snap regardless of this flag.
    pub pixel_snap: bool,
    /// Monitor refresh rate in millihertz (Hz × 1000), or `None` when
    /// the host can't determine it (headless, unmapped window, VRR).
    /// Read only by repaint-wake coalescing (`coalesce_dt_for_refresh`
    /// turns it into the scheduler's floor); it is *not* a projection
    /// input, so — like `pixel_snap` — it stays out of `logical_rect`
    /// and the cascade fingerprint and never forces a relayout.
    pub refresh_millihertz: Option<u32>,
}

impl Default for Display {
    fn default() -> Self {
        Self {
            physical: UVec2::ZERO,
            scale_factor: 1.0,
            pixel_snap: true,
            refresh_millihertz: None,
        }
    }
}

impl Display {
    /// Build from physical surface size + scale factor. `pixel_snap`
    /// defaults to `true` — flip with struct update if you need the
    /// off variant: `Display { pixel_snap: false, ..Display::from_physical(...) }`.
    pub fn from_physical(physical: UVec2, scale_factor: f32) -> Self {
        Self {
            physical,
            scale_factor,
            pixel_snap: true,
            refresh_millihertz: None,
        }
    }

    /// Logical surface size = physical / scale_factor.
    pub fn logical_size(&self) -> Size {
        Size::new(
            self.physical.x as f32 / self.scale_factor,
            self.physical.y as f32 / self.scale_factor,
        )
    }

    /// Logical surface rect at origin (0, 0) — pass to
    /// [`Ui::layout`](crate::ui::Ui::layout) and
    /// [`Ui::damage_filter`](crate::ui::Ui::damage_filter).
    pub fn logical_rect(&self) -> Rect {
        Rect {
            min: glam::Vec2::ZERO,
            size: self.logical_size(),
        }
    }

    /// True when `other` rasterizes identically: same physical size,
    /// scale factor, and pixel snapping. `logical_rect` equality is NOT
    /// enough — a DPI-monitor move scales `physical` and `scale_factor`
    /// proportionally, leaving the logical rect bit-identical while the
    /// swapchain is reconfigured to a new pixel size. `refresh_millihertz`
    /// is pacing-only and deliberately excluded.
    pub fn raster_eq(&self, other: &Display) -> bool {
        self.physical == other.physical
            && self.scale_factor == other.scale_factor
            && self.pixel_snap == other.pixel_snap
    }
}
