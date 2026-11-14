//! [`ImagePaint`]: a registered image over a rect, a circle or a triangle. Lowers to
//! a `ShapeRecord::Closed` of `ClosedPaint::Image`.

use crate::image_registry::image_handle::ImageHandle;
use crate::primitives::geometry::fit::Fit;
use crate::primitives::math::domain;
use crate::primitives::math::domain::vec2;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::image::{ImageDownsample, ImageFilter, ImagePlacement};
use crate::shape::PaintedBy;
use crate::shape::circle::CircleShape;
use crate::shape::draw::{ClosedPaintRef, Draw};
use crate::shape::geometry::closed_geometry::ClosedGeometry;
use crate::shape::rect::RectShape;
use crate::shape::sealed;
use crate::shape::triangle::TriangleShape;
use glam::Vec2;

/// A registered [`ImageHandle`] painted over a [`RectShape`], a [`CircleShape`] or a
/// [`TriangleShape`] ([`Ui::add_shape`](crate::Ui::add_shape)), as Skia's `SkImageShader` fills a
/// shape. The image fits the rect, a circle's square or a triangle's covering rect, by its
/// [`Fit`], or tiles across it ([`Self::tile`]), and the shape clips it, as CSS's `border-radius`
/// clips an `object-fit` image: a contained image shows nothing past its own rect.
#[derive(Clone, Debug)]
#[must_use]
pub struct ImagePaint {
    pub(crate) handle: ImageHandle,
    pub(crate) placement: ImagePlacement,
    pub(crate) min_filter: ImageFilter,
    pub(crate) mag_filter: ImageFilter,
    pub(crate) downsample: ImageDownsample,
    pub(crate) tint: RgbaF32,
}

impl ImagePaint {
    /// The image `handle`, stretched to the rect ([`Fit::Fill`]), linear-filtered, untinted.
    pub fn new(handle: ImageHandle) -> Self {
        Self {
            handle,
            placement: ImagePlacement::FILL,
            min_filter: ImageFilter::default(),
            mag_filter: ImageFilter::default(),
            downsample: ImageDownsample::default(),
            tint: RgbaF32::WHITE,
        }
    }

    /// Fit the image in the rect by its intrinsic size; [`Fit::Fill`] by default. `Natural`
    /// paints it unscaled and centered, `Cover` crops it by UV to the rect.
    pub const fn fit(mut self, fit: Fit) -> Self {
        self.placement = ImagePlacement::Fit(fit);
        self
    }

    /// Repeat the image across the rect, its intrinsic size ignored: `scale` repeats on each axis,
    /// `offset` the scroll phase, both in UV, which the shader wraps.
    ///
    /// # Panics
    ///
    /// Panics unless `offset` is an [offset](crate::widget::domain::is_offset) and each axis of
    /// `scale` is [positive](crate::widget::domain::is_positive).
    #[track_caller]
    pub const fn tile(mut self, offset: Vec2, scale: Vec2) -> Self {
        self.placement = ImagePlacement::Tile {
            offset: vec2::offset(offset),
            scale: Vec2::new(domain::positive(scale.x), domain::positive(scale.y)),
        };
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
    /// Panics unless `tint` is a [color](crate::widget::domain::is_color).
    #[track_caller]
    pub const fn tint(mut self, tint: RgbaF32) -> Self {
        self.tint = domain::color(tint);
        self
    }
}

impl PaintedBy<ImagePaint> for RectShape {}

/// An image over a rect, its corners rounding it.
impl sealed::DrawWith<ImagePaint> for RectShape {
    type Prepared = ImagePaint;

    fn prepare(paint: ImagePaint) -> ImagePaint {
        paint
    }

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
    type Prepared = ImagePaint;

    fn prepare(paint: ImagePaint) -> ImagePaint {
        paint
    }

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
    type Prepared = ImagePaint;

    fn prepare(paint: ImagePaint) -> ImagePaint {
        paint
    }

    fn check(&self, _paint: &ImagePaint) {}

    fn draw<'a>(&'a self, paint: &'a ImagePaint) -> Draw<'a> {
        Draw::Closed {
            geometry: ClosedGeometry::Triangle(self.geometry()),
            paint: ClosedPaintRef::Image(paint),
        }
    }
}
