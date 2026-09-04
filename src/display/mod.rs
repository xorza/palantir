//! The surface a frame paints onto: its physical size, the two factors
//! that convert logical to physical, and the refresh rate the wake
//! scheduler paces against.
//!
//! **Two factors, one product.** [`Display::system_scale`] is what the
//! platform reported and [`Display::user_scale`] is what the application
//! chose on top of it. Everything that rasterizes multiplies by
//! [`Display::scale_factor`], their product. Everything that talks back
//! to the window manager — a saved window size, a size handed to winit —
//! uses `system_scale` alone, because that is the space those numbers are
//! read in. [`Display::system_logical_size`] is the one that answers it.
//!
//! The system factor is screened at the door — see
//! [`sanitize_system_scale`](crate::display::sanitize_system_scale) — so
//! nothing downstream divides by a value the platform never promised. The
//! user factor carries its range in its type.

pub(crate) mod user_scale;

use crate::display::user_scale::UserScale;
use crate::primitives::approx::EPS;
use crate::primitives::rect::Rect;
use crate::primitives::size::Size;
use glam::{UVec2, Vec2};

#[inline]
pub(crate) const fn scale_factor_is_valid(scale_factor: f32) -> bool {
    scale_factor.is_finite() && scale_factor >= EPS
}

/// `system_scale` when the platform reported a usable one, else `1.0`.
///
/// The windowed host's door. Winit hands over an `f64` it promises
/// nothing about, and a bad one divides every pointer coordinate into
/// nonsense several layers before the [`scale_factor_is_valid`] assert
/// that would name it. The offscreen host rejects at its own door for
/// the same reason, and this is the windowed side of that contract —
/// one screen where the value enters, rather than a floor at each
/// division downstream. Gated with that host: an offscreen-only build
/// has no `f64` arriving from a platform to screen.
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

/// Display state for the current output: read by the renderer at
/// submit time, by hosts computing the logical surface rect for
/// layout, and by the repaint scheduler for frame pacing. Carries the
/// surface's physical pixel size, the two scale factors, the
/// snap-to-physical-pixel-edge flag, and the monitor's refresh rate.
///
/// The driving host mints this each frame through `WindowDriver::display`,
/// from the window's surface config, system scale and monitor refresh, then
/// passes it to `WindowDriver::cpu_frame`.
/// Changes that alter rasterized output are detected via [`Self::raster_eq`]
/// (physical size, both scales, pixel snapping — a DPI-monitor move keeps
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
    /// The device pixel ratio the platform reported (e.g. `2.0` on a 2×
    /// retina display). Must be finite and at least `approx::EPS`; host
    /// boundaries validate external values and `Ui::frame` checks the
    /// invariant on the product.
    ///
    /// Not the number to multiply a painted length by — that is
    /// [`Self::scale_factor`]. This one is the window manager's space,
    /// and its readers are the ones that hand a size back to it.
    pub system_scale: f32,
    /// The application's own scale, multiplied onto [`Self::system_scale`].
    /// Set through [`Ui::set_user_scale`](crate::Ui::set_user_scale), which
    /// is app-global — the host mints every window's `Display` from the
    /// one value.
    pub user_scale: UserScale,
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
            system_scale: 1.0,
            user_scale: UserScale::ONE,
            pixel_snap: true,
            refresh_millihertz: None,
        }
    }
}

impl Display {
    /// Build from physical surface size + system scale, no user scale,
    /// snapping on and no declared refresh rate.
    ///
    /// For an embedder assembling a frame itself. Palantir's own hosts do
    /// not use it: `pixel_snap` and `user_scale` are theirs to supply, and
    /// a `Display` built here would silently take these defaults instead —
    /// so both hosts mint theirs through the `WindowDriver` that owns both,
    /// and that is the only place either reaches a frame.
    pub fn from_physical(physical: UVec2, system_scale: f32) -> Self {
        Self {
            physical,
            system_scale,
            ..Default::default()
        }
    }

    /// Logical→physical for everything the application draws: the system
    /// factor times the user's own.
    ///
    /// **The one number the render path multiplies by.** A caller reaching
    /// for either field instead has picked one of the two spaces, and
    /// wants to be sure it is the one it means.
    #[inline]
    pub fn scale_factor(&self) -> f32 {
        self.user_scale.applied_to(self.system_scale)
    }

    /// Logical surface size the UI is laid out in = physical /
    /// [`Self::scale_factor`]. A larger user scale leaves less of it,
    /// which is the whole of what scaling up does to layout.
    pub fn logical_size(&self) -> Size {
        self.divided_by(self.scale_factor())
    }

    /// Surface size in the *window manager's* logical pixels = physical /
    /// [`Self::system_scale`].
    ///
    /// What a size handed back to the platform is read in: winit's
    /// `LogicalSize`, and so
    /// [`WindowConfig::inner_size`](crate::WindowConfig::inner_size). Equal
    /// to [`Self::logical_size`] only while the user scale is `1.0`, which
    /// is exactly why the two are named apart — a round trip through the
    /// wrong one shrinks the window by the user scale on every launch.
    pub fn system_logical_size(&self) -> Size {
        self.divided_by(self.system_scale)
    }

    fn divided_by(&self, scale: f32) -> Size {
        Size::new(
            self.physical.x as f32 / scale,
            self.physical.y as f32 / scale,
        )
    }

    /// Logical surface rect at origin (0, 0), used by layout and damage
    /// filtering.
    pub fn logical_rect(&self) -> Rect {
        Rect {
            min: Vec2::ZERO,
            size: self.logical_size(),
        }
    }

    /// True when `other` rasterizes identically: same physical size,
    /// same two scales, same pixel snapping. `logical_rect` equality is
    /// NOT enough — a DPI-monitor move scales `physical` and
    /// `system_scale` proportionally, leaving the logical rect
    /// bit-identical while the swapchain is reconfigured to a new pixel
    /// size. `refresh_millihertz` is pacing-only and deliberately
    /// excluded.
    ///
    /// The two scales are compared as themselves rather than through
    /// [`Self::scale_factor`]: the product cannot tell a 2× monitor from
    /// a 1× monitor at 200%, and those differ in what the window manager
    /// is told even when every painted pixel matches.
    pub fn raster_eq(&self, other: &Display) -> bool {
        self.physical == other.physical
            && self.system_scale == other.system_scale
            && self.user_scale == other.user_scale
            && self.pixel_snap == other.pixel_snap
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The product is what layout divides by, and the system factor alone
    /// is what the window manager is told. At 2× and 125% a 1000-px
    /// surface is 400 logical px to the UI and 500 to the platform.
    #[test]
    fn the_two_spaces_divide_by_different_factors() {
        let display = Display {
            user_scale: UserScale::new(1.25),
            ..Display::from_physical(UVec2::new(1000, 600), 2.0)
        };
        assert_eq!(display.scale_factor(), 2.5);
        assert_eq!(display.logical_size(), Size::new(400.0, 240.0));
        assert_eq!(display.system_logical_size(), Size::new(500.0, 300.0));
        assert_eq!(display.logical_rect(), Rect::new(0.0, 0.0, 400.0, 240.0));
    }

    /// `from_physical` leaves the user scale at `ONE`, so the two spaces
    /// coincide until something sets it.
    #[test]
    fn without_a_user_scale_the_two_spaces_are_one() {
        let display = Display::from_physical(UVec2::new(800, 600), 2.0);
        assert_eq!(display.user_scale, UserScale::ONE);
        assert_eq!(display.scale_factor(), 2.0);
        assert_eq!(display.logical_size(), display.system_logical_size());
    }

    /// A user-scale move rasterizes differently, so it must fail
    /// `raster_eq` and force the full repaint that follows from it.
    #[test]
    fn raster_eq_sees_a_user_scale_move() {
        let base = Display::from_physical(UVec2::new(800, 600), 2.0);
        let zoomed = Display {
            user_scale: UserScale::new(1.25),
            ..base
        };
        assert!(base.raster_eq(&base));
        assert!(!base.raster_eq(&zoomed));
    }

    /// Same painted pixels, different window-manager space: 2× at 100%
    /// and 1× at 200% share a `scale_factor` and must still compare
    /// unequal, because `system_logical_size` differs.
    #[test]
    fn raster_eq_splits_two_displays_that_share_a_product() {
        let retina = Display::from_physical(UVec2::new(800, 600), 2.0);
        let scaled_up = Display {
            user_scale: UserScale::new(2.0),
            ..Display::from_physical(UVec2::new(800, 600), 1.0)
        };
        assert_eq!(retina.scale_factor(), scaled_up.scale_factor());
        assert!(!retina.raster_eq(&scaled_up));
        assert_ne!(
            retina.system_logical_size(),
            scaled_up.system_logical_size()
        );
    }
}
