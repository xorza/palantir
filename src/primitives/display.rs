use super::{Rect, Size};
use glam::UVec2;

/// Display state read by the renderer at submit time and by hosts
/// computing the logical surface rect for layout. Carries everything
/// the renderer needs to project logical pixels to physical: the
/// surface's physical pixel size, the DPR scale factor, and the
/// snap-to-physical-pixel-edge flag.
///
/// The host calls [`Ui::set_display`](crate::ui::Ui::set_display) on
/// init and on every winit event that changes one of these (resize,
/// scale-factor change). Each call is change-detected — re-setting
/// the same value is a free no-op.
///
/// Group exists so future rasterization knobs (sRGB correction, MSAA,
/// gamma) have a clear home.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Display {
    /// Physical surface size in pixels — same value the host hands
    /// to `wgpu::SurfaceConfiguration { width, height, .. }`.
    pub physical: UVec2,
    /// Logical→physical conversion factor (e.g. `2.0` on a 2× retina
    /// display). Must be `≥ f32::EPSILON`; `Ui::set_display` asserts.
    pub scale_factor: f32,
    /// Whether the renderer snaps rect edges to integer physical
    /// pixels. Default `true` — sharper edges, no half-pixel blur.
    pub pixel_snap: bool,
}

impl Default for Display {
    fn default() -> Self {
        Self {
            physical: UVec2::ZERO,
            scale_factor: 1.0,
            pixel_snap: true,
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
}
