//! [`IconPaint`]: a baked icon over a rect. Lowers to a `ShapeRecord::Closed` of `ClosedPaint::Icon`.

use crate::primitives::geometry::fit::Fit;
use crate::primitives::identity::icon_handle::IconHandle;
use crate::primitives::math::domain;
use crate::primitives::paint::color::RgbaF32;
use crate::shape::PaintedBy;
use crate::shape::draw::ClosedPaintRef;
use crate::shape::draw::Draw;
use crate::shape::geometry::closed_geometry::ClosedGeometry;
use crate::shape::rect::RectShape;
use crate::shape::sealed;

/// A baked SVG icon painted over a [`RectShape`] ([`Ui::add_shape`](crate::Ui::add_shape)), rasterized at its exact
/// physical size. `tint` applies whole to a **tintable** icon (single-color
/// paints) but only as alpha to a **color** icon.
#[derive(Clone, Copy, Debug)]
#[must_use]
pub struct IconPaint {
    pub(crate) handle: IconHandle,
    pub(crate) fit: Fit,
    pub(crate) tint: RgbaF32,
    pub(crate) desaturate: bool,
}

impl IconPaint {
    /// The icon `handle`, fit by [`Fit::Contain`], untinted, in its own colors.
    pub const fn new(handle: IconHandle) -> Self {
        Self {
            handle,
            fit: Fit::Contain,
            tint: RgbaF32::WHITE,
            desaturate: false,
        }
    }

    /// Fit the icon's artwork into the rect by `fit`; [`Fit::Contain`] by default.
    pub const fn fit(mut self, fit: Fit) -> Self {
        self.fit = fit;
        self
    }

    /// Multiplies the icon by `tint`; see the type docs.
    ///
    /// # Panics
    ///
    /// Panics unless `tint` is a [color](crate::widget::domain::color).
    #[track_caller]
    pub const fn tint(mut self, tint: RgbaF32) -> Self {
        self.tint = domain::color(tint);
        self
    }

    /// Draws a **color** icon in grayscale; no effect on a tintable icon.
    pub const fn desaturate(mut self, desaturate: bool) -> Self {
        self.desaturate = desaturate;
        self
    }
}

impl PaintedBy<IconPaint> for RectShape {}

/// An icon over a rect, square-cornered and covering its inside.
impl sealed::DrawWith<IconPaint> for RectShape {
    /// # Panics
    ///
    /// Unless the rect is square-cornered and covers its inside, which an icon's pipeline draws.
    #[track_caller]
    fn check(&self, _paint: &IconPaint) {
        self.assert_plain("an icon");
    }

    fn draw<'a>(&'a self, paint: &'a IconPaint) -> Draw<'a> {
        Draw::Closed {
            geometry: ClosedGeometry::Box(self.geometry()),
            paint: ClosedPaintRef::Icon(paint),
        }
    }
}
