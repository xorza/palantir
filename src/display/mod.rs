//! The surface a frame paints onto: physical size, the two logical-to-physical
//! factors, and the refresh rate the wake scheduler paces against.
//!
//! [`Display::system_scale`] is what the platform reported and
//! [`Display::user_scale`] what the application chose on top. Rasterizing
//! multiplies by [`Display::scale_factor`], their product; anything talking
//! back to the window manager (a saved window size, a size handed to winit)
//! uses `system_scale` alone, via [`Display::system_logical_size`]. The system
//! factor is screened by [`sanitize_system_scale`](crate::display::sanitize_system_scale)
//! so nothing divides by a value the platform never promised; the user factor
//! carries its range in its type.

pub(crate) mod user_scale;

use crate::display::user_scale::UserScale;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::geometry::size::Size;
use crate::primitives::math::domain::EPS;
use glam::{UVec2, Vec2};

/// What every entry point for a scale factor says when
/// [`scale_factor_is_valid`] fails.
pub(crate) const SCALE_RULE: &str = "a scale factor must be finite and at least 1e-4";

#[inline]
pub(crate) const fn scale_factor_is_valid(scale_factor: f32) -> bool {
    scale_factor.is_finite() && scale_factor >= EPS
}

/// `system_scale` when the platform reported a usable one, else `1.0`.
///
/// The windowed host's door: winit's `f64` promises nothing, and a bad one
/// would corrupt every pointer coordinate layers before the
/// [`scale_factor_is_valid`] assert. Gated with that host.
#[cfg(feature = "winit")]
#[inline]
pub(crate) fn sanitize_system_scale(system_scale: f64) -> f32 {
    let system_scale = system_scale as f32;
    if scale_factor_is_valid(system_scale) {
        system_scale
    } else {
        tracing::warn!(system_scale, "display.system_scale_rejected");
        1.0
    }
}

/// Display state for the current output, read by the renderer at submit, by
/// hosts computing the logical surface rect, and by the repaint scheduler.
///
/// The host mints it each frame through `WindowDriver::display` and passes it
/// to `WindowDriver::cpu_frame`. Changes to rasterized output are detected by
/// [`Self::raster_eq`]; `refresh_millihertz` is pacing-only and never forces a
/// repaint.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Display {
    /// Physical surface size in pixels, as in `wgpu::SurfaceConfiguration`.
    pub physical: UVec2,
    /// The device pixel ratio the platform reported (`2.0` on a 2× display).
    /// Finite and at least `domain::EPS`; hosts validate external values and
    /// `Ui::frame` checks the product. For the window manager's space; paint
    /// with [`Self::scale_factor`].
    pub system_scale: f32,
    /// The application's own scale, multiplied onto [`Self::system_scale`].
    /// Set through [`Ui::set_user_scale`](crate::Ui::set_user_scale), which is
    /// app-global.
    pub user_scale: UserScale,
    /// Whether the composer snaps painted geometry edges (quads, shadows,
    /// images, text bounds, clip scissors) to integer physical pixels. Mesh,
    /// curve and polyline vertices and corner radii never snap. Damage
    /// scissors always snap, as `set_scissor_rect` takes `u32`.
    pub pixel_snap: bool,
    /// Monitor refresh rate in millihertz, or `None` when unknown (headless,
    /// unmapped window, VRR). Read only by repaint-wake coalescing; it never
    /// forces a relayout.
    pub refresh_millihertz: Option<u32>,
}

impl Default for Display {
    fn default() -> Self {
        Self {
            physical: UVec2::ZERO,
            system_scale: 1.0,
            user_scale: UserScale::ONE,
            pixel_snap: true,
            refresh_millihertz: None,
        }
    }
}

impl Display {
    /// Build from physical size and system scale, with no user scale,
    /// snapping on and no refresh rate.
    ///
    /// For an embedder assembling a frame itself; Palantir's hosts mint theirs
    /// through `WindowDriver`, which supplies `pixel_snap` and `user_scale`.
    /// # Panics
    ///
    /// Panics unless `system_scale` is finite and at least `1e-4`.
    #[track_caller]
    pub const fn from_physical(physical: UVec2, system_scale: f32) -> Self {
        Self {
            physical,
            system_scale: {
                assert!(scale_factor_is_valid(system_scale), "{}", SCALE_RULE);
                system_scale
            },
            user_scale: UserScale::ONE,
            pixel_snap: true,
            refresh_millihertz: None,
        }
    }

    /// Logical to physical for everything the application draws: the system
    /// factor times the user's. The one number the render path multiplies by.
    #[inline]
    pub const fn scale_factor(&self) -> f32 {
        self.user_scale.applied_to(self.system_scale)
    }

    /// Logical surface size the UI is laid out in: physical /
    /// [`Self::scale_factor`].
    pub fn logical_size(&self) -> Size {
        self.divided_by(self.scale_factor())
    }

    /// Surface size in the window manager's logical pixels: physical /
    /// [`Self::system_scale`]. What winit's `LogicalSize` and
    /// [`WindowConfig::with_inner_size`](crate::WindowConfig::with_inner_size)
    /// read; round-tripping through [`Self::logical_size`] would shrink the
    /// window by the user scale each launch.
    pub fn system_logical_size(&self) -> Size {
        self.divided_by(self.system_scale)
    }

    fn divided_by(&self, scale: f32) -> Size {
        Size::new(
            self.physical.x as f32 / scale,
            self.physical.y as f32 / scale,
        )
    }

    /// Logical surface rect at the origin, used by layout and damage.
    pub fn logical_rect(&self) -> Rect {
        Rect {
            min: Vec2::ZERO,
            size: self.logical_size(),
        }
    }

    /// True when `other` rasterizes identically: same physical size, scales
    /// and pixel snapping. `logical_rect` equality is not enough, since a
    /// DPI-monitor move leaves it bit-identical while the swapchain resizes.
    /// `refresh_millihertz` is excluded. The scales are compared as
    /// themselves, as the product cannot tell a 2× monitor from a 1× one at
    /// 200%.
    pub fn raster_eq(&self, other: &Display) -> bool {
        self.physical == other.physical
            && self.system_scale == other.system_scale
            && self.user_scale == other.user_scale
            && self.pixel_snap == other.pixel_snap
    }
}

#[cfg(test)]
mod tests;
