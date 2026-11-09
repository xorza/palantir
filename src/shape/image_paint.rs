//! [`ImagePaint`]: a registered image over a rect. Lowers to `ShapeRecord::Image`.

use crate::image_registry::image_handle::ImageHandle;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::math::domain;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::image::{ImageDownsample, ImageFilter, ImageFit};
use crate::shape::paint::image_source::ImageSource;
use crate::shape::record::ShapeRecord;
use crate::shape::record_store::RecordStore;
use crate::shape::rect::RectShape;
use crate::shape::sealed::{self, Resolve};

/// A registered [`ImageHandle`] painted over a [`RectShape`]
/// ([`Ui::add_shape`](crate::Ui::add_shape)), as Skia's `SkImageShader` fills a shape.
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

/// An image over a rect, square-cornered and covering its inside.
impl sealed::LowerShape for (RectShape, ImagePaint) {
    /// # Panics
    ///
    /// Unless the rect is square-cornered and covers its inside, which an image's pipeline draws.
    #[track_caller]
    fn validate(&self) {
        self.0.assert_plain("an image");
    }

    fn lower(self, _resolve: Resolve<'_>, _store: &mut RecordStore) -> Option<ShapeRecord> {
        let (
            RectShape {
                local_rect,
                corners: _,
                coverage: _,
            },
            ImagePaint {
                handle,
                fit,
                min_filter,
                mag_filter,
                downsample,
                tint,
            },
        ) = self;
        if local_rect.is_some_and(Rect::is_paint_empty) || tint.is_noop() {
            return None;
        }
        Some(ShapeRecord::Image {
            local_rect,
            tint: tint.to_f16(),
            source: ImageSource::Texture {
                id: handle.id(),
                size: handle.size(),
                generation: handle.generation(),
            },
            fit,
            min_filter,
            mag_filter,
            downsample,
        })
    }
}
