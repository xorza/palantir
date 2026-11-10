//! [`ImagePaint`]: a registered image over a rect, a circle or a triangle. Lowers to
//! a `ShapeRecord::Closed` of `ClosedPaint::Image`.

use crate::image_registry::image_handle::ImageHandle;
use crate::primitives::math::domain;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::image::{ImageDownsample, ImageFilter, ImageFit};
use crate::shape::PaintedBy;
use crate::shape::circle::CircleShape;
use crate::shape::draw::{ClosedPaintRef, Draw};
use crate::shape::geometry::closed_geometry::ClosedGeometry;
use crate::shape::rect::RectShape;
use crate::shape::sealed;
use crate::shape::triangle::TriangleShape;

/// A registered [`ImageHandle`] painted over a [`RectShape`], a [`CircleShape`] or a
/// [`TriangleShape`] ([`Ui::add_shape`](crate::Ui::add_shape)), as Skia's `SkImageShader` fills a
/// shape. The image fits the rect, a circle's square or a triangle's covering rect, by its
/// [`ImageFit`], and the shape clips it, as CSS's `border-radius` clips an `object-fit` image: a
/// contained image shows nothing past its own rect.
#[derive(Clone, Debug)]
#[must_use]
pub struct ImagePaint {
    pub(crate) handle: ImageHandle,
    pub(crate) fit: ImageFit,
    pub(crate) min_filter: ImageFilter,
    pub(crate) mag_filter: ImageFilter,
    pub(crate) downsample: ImageDownsample,
    pub(crate) tint: RgbaF32,
}

impl ImagePaint {
    /// The image `handle`, fit by [`ImageFit::default`], linear-filtered, untinted.
    pub fn new(handle: ImageHandle) -> Self {
        Self {
            handle,
            fit: ImageFit::default(),
            min_filter: ImageFilter::default(),
            mag_filter: ImageFilter::default(),
            downsample: ImageDownsample::default(),
            tint: RgbaF32::WHITE,
        }
    }

    /// How the image is placed inside the rect.
    ///
    /// # Panics
    ///
    /// Panics unless a [`ImageFit::Tile`]'s `offset` is an [offset](crate::widget::domain::offset) and each axis of its `scale` is [positive](crate::widget::domain::positive).
    #[track_caller]
    pub const fn fit(mut self, fit: ImageFit) -> Self {
        fit.validate();
        self.fit = fit;
        self
    }

    /// Filtering while the image is drawn smaller than its intrinsic size.
    pub const fn min_filter(mut self, min_filter: ImageFilter) -> Self {
        self.min_filter = min_filter;
        self
    }

    /// Filtering while drawn larger; [`ImageFilter::Nearest`] keeps pixel art crisp.
    pub const fn mag_filter(mut self, mag_filter: ImageFilter) -> Self {
        self.mag_filter = mag_filter;
        self
    }

    /// Extra taps where the image minifies instead of one bilinear tap; see [`ImageDownsample`]. Off by default, worth it only for shrinking images with fine detail.
    pub const fn downsample(mut self, downsample: ImageDownsample) -> Self {
        self.downsample = downsample;
        self
    }

    /// Multiplied onto every texel. White leaves the image alone.
    ///
    /// # Panics
    ///
    /// Panics unless `tint` is a [color](crate::widget::domain::color).
    #[track_caller]
    pub const fn tint(mut self, tint: RgbaF32) -> Self {
        self.tint = domain::color(tint);
        self
    }
}

impl PaintedBy<ImagePaint> for RectShape {}

/// An image over a rect, its corners rounding it.
impl sealed::DrawWith<ImagePaint> for RectShape {
    fn check(&self, _paint: &ImagePaint) {}

    fn draw<'a>(&'a self, paint: &'a ImagePaint) -> Draw<'a> {
        Draw::Closed {
            geometry: ClosedGeometry::Box(self.geometry()),
            paint: ClosedPaintRef::Image(paint),
        }
    }
}

impl PaintedBy<ImagePaint> for CircleShape {}

/// An image over a circle, fit to the circle's square and clipped to the circle.
impl sealed::DrawWith<ImagePaint> for CircleShape {
    fn check(&self, _paint: &ImagePaint) {}

    fn draw<'a>(&'a self, paint: &'a ImagePaint) -> Draw<'a> {
        Draw::Closed {
            geometry: ClosedGeometry::Box(self.geometry()),
            paint: ClosedPaintRef::Image(paint),
        }
    }
}

impl PaintedBy<ImagePaint> for TriangleShape {}

/// An image over a triangle, fit to its covering rect and clipped to it.
impl sealed::DrawWith<ImagePaint> for TriangleShape {
    fn check(&self, _paint: &ImagePaint) {}

    fn draw<'a>(&'a self, paint: &'a ImagePaint) -> Draw<'a> {
        Draw::Closed {
            geometry: ClosedGeometry::Triangle(self.geometry()),
            paint: ClosedPaintRef::Image(paint),
        }
    }
}
