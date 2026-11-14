//! The surface a frame paints onto: physical size, the two logical-to-physical
//! factors, and the refresh rate the wake scheduler paces against.
//!
//! [`Display::system_scale`] is what the platform reported and
//! [`Display::user_scale`] what the application chose on top. Rasterizing
//! multiplies by [`Display::scale_factor`], their product; anything talking
//! back to the window manager (a saved window size, a size handed to winit)
//! uses `system_scale` alone, via [`Display::system_logical_size`]. Each factor
//! carries its range in its type, and the two ranges keep the product finite and at
//! least `1e-4`.

pub(crate) mod frame_stamp;
pub(crate) mod system_scale;
pub(crate) mod user_scale;

use crate::display::system_scale::SystemScale;
use crate::display::user_scale::UserScale;
use crate::primitives::geometry::rect::{EdgeRounding, Rect};
use crate::primitives::geometry::size::Size;
use crate::primitives::math::domain::EPS;
use crate::primitives::math::num::Vec2Ext;
use glam::{UVec2, Vec2};
use std::num::NonZeroU32;

#[inline]
pub(crate) const fn scale_factor_is_valid(scale_factor: f32) -> bool {
    scale_factor.is_finite() && scale_factor >= EPS
}

/// Display state for the current output, read by the renderer at submit, by
/// hosts computing the logical surface rect, and by the repaint scheduler.
///
/// A host's window driver mints it each frame from the frame's target, scale and refresh rate,
/// and the host-wide user scale and pixel snapping. Changes to rasterized output are detected by
/// `raster_eq`; the refresh rate is pacing-only and never forces a repaint. Handed out read-only,
/// so its state is read through getters.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Display {
    pub(crate) physical: UVec2,
    pub(crate) system_scale: SystemScale,
    pub(crate) user_scale: UserScale,
    pub(crate) pixel_snap: bool,
    pub(crate) refresh_millihertz: Option<NonZeroU32>,
}

impl Default for Display {
    fn default() -> Self {
        Self {
            physical: UVec2::ZERO,
            system_scale: SystemScale::ONE,
            user_scale: UserScale::ONE,
            pixel_snap: true,
            refresh_millihertz: None,
        }
    }
}

impl Display {
    /// Physical surface size in pixels, as in `wgpu::SurfaceConfiguration`.
    #[inline]
    pub const fn physical_size(&self) -> UVec2 {
        self.physical
    }

    /// The device pixel ratio the platform reported (`2.0` on a 2× display). For the window
    /// manager's space; paint with [`Self::scale_factor`].
    #[inline]
    pub const fn system_scale(&self) -> SystemScale {
        self.system_scale
    }

    /// The application's own scale, multiplied onto [`Self::system_scale`]. Set through
    /// [`Ui::set_user_scale`](crate::Ui::set_user_scale), which is app-global.
    #[inline]
    pub const fn user_scale(&self) -> UserScale {
        self.user_scale
    }

    /// Whether the composer snaps painted geometry edges (quads, shadows, images, text bounds,
    /// clip scissors) to integer physical pixels, each in its own frame before its transform, and
    /// a scroll lands its pan on them. Mesh, curve and polyline vertices and corner radii never
    /// snap. Damage scissors always snap, as `set_scissor_rect` takes `u32`.
    #[inline]
    pub const fn pixel_snap(&self) -> bool {
        self.pixel_snap
    }

    /// Monitor refresh rate in millihertz, or `None` when unknown (headless, unmapped window,
    /// VRR). Read only by wake pacing (repaint coalescing, reduced frame rate); it never forces a
    /// relayout.
    #[inline]
    pub const fn refresh_millihertz(&self) -> Option<NonZeroU32> {
        self.refresh_millihertz
    }

    /// Where the composer lands painted geometry edges, per [`Self::pixel_snap`].
    pub(crate) const fn edge_rounding(&self) -> EdgeRounding {
        if self.pixel_snap {
            EdgeRounding::WholePixels
        } else {
            EdgeRounding::Exact
        }
    }

    /// `translation`, in logical px, landed where an edge lands: on whole physical px where
    /// [`Self::pixel_snap`] holds, else as it is. For a pan or a scroll offset kept exact and drawn
    /// landed, as Blink keeps a scroll offset on whole pixels: the composer places content by its
    /// transform exactly, so unzoomed content then stays on its pixels at any offset.
    pub fn land_translation(&self, translation: Vec2) -> Vec2 {
        match self.edge_rounding() {
            EdgeRounding::WholePixels => {
                let scale = self.scale_factor();
                (translation * scale).fast_round() / scale
            }
            EdgeRounding::Exact => translation,
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
        self.divided_by(self.system_scale.get())
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
    pub(crate) fn raster_eq(&self, other: &Self) -> bool {
        self.physical == other.physical
            && self.system_scale == other.system_scale
            && self.user_scale == other.user_scale
            && self.pixel_snap == other.pixel_snap
    }
}

#[cfg(feature = "internals")]
pub(crate) mod internals {
    use crate::display::Display;
    use crate::display::system_scale::SystemScale;
    use crate::display::user_scale::UserScale;
    use glam::UVec2;

    impl Display {
        /// Build from physical size and system scale, with no user scale,
        /// snapping on and no refresh rate.
        ///
        /// # Panics
        ///
        /// Panics outside [`SystemScale`]'s range.
        #[track_caller]
        pub(crate) fn from_physical(physical: UVec2, system_scale: f32) -> Self {
            Self {
                physical,
                system_scale: SystemScale::new(system_scale).unwrap_or_else(|| {
                    panic!(
                        "a system scale must be from {:e} to {:e}, not {system_scale}",
                        SystemScale::MIN,
                        SystemScale::MAX
                    )
                }),
                user_scale: UserScale::ONE,
                pixel_snap: true,
                refresh_millihertz: None,
            }
        }
    }
}

#[cfg(test)]
mod tests;
