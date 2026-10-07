//! The textured-rectangle builder. Lowers to `ShapeRecord::Image`.

use crate::primitives::geometry::rect::Rect;
use crate::primitives::math::domain;
use crate::primitives::math::nan::NanCheck;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::image::{ImageDownsample, ImageFilter, ImageFit};
use crate::renderer::image_registry::image_handle::ImageHandle;
use crate::scene::record_store::RecordStore;
use crate::shape::paint::image_source::ImageSource;
use crate::shape::record::ShapeRecord;
use crate::shape::sealed;

/// Textured rectangle painted from a registered [`ImageHandle`].
#[derive(Clone, Debug)]
#[must_use]
pub struct ImageShape {
    pub(crate) handle: ImageHandle,
    pub(crate) local_rect: Option<Rect>,
    pub(crate) fit: ImageFit,
    pub(crate) min_filter: ImageFilter,
    pub(crate) mag_filter: ImageFilter,
    pub(crate) downsample: ImageDownsample,
    pub(crate) tint: RgbaF32,
}

impl ImageShape {
    pub(super) fn new(handle: ImageHandle) -> Self {
        Self {
            handle,
            local_rect: None,
            fit: ImageFit::default(),
            min_filter: ImageFilter::default(),
            mag_filter: ImageFilter::default(),
            downsample: ImageDownsample::default(),
            tint: RgbaF32::WHITE,
        }
    }
}

impl ImageShape {
    /// Paint into `rect` in owner-relative coords instead of the owner's arranged rect.
    ///
    /// # Panics
    ///
    /// Panics unless every component of `rect` is an [offset](crate::widget::domain::offset).
    #[track_caller]
    pub const fn at(mut self, rect: Rect) -> Self {
        rect.validate();
        self.local_rect = Some(rect);
        self
    }

    /// How the image is placed inside the paint rect.
    pub const fn fit(mut self, fit: ImageFit) -> Self {
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
    /// Panics unless `tint` is a [colour](crate::widget::domain::color).
    #[track_caller]
    pub const fn tint(mut self, tint: RgbaF32) -> Self {
        self.tint = domain::color(tint);
        self
    }
}

impl sealed::LowerShape for ImageShape {
    fn is_noop(&self) -> bool {
        self.local_rect.is_some_and(Rect::is_paint_empty) || self.tint.is_noop()
    }

    fn has_nan(&self) -> bool {
        self.local_rect.has_nan() || self.tint.has_nan() || self.fit.has_nan()
    }

    fn lower(self, _store: &mut RecordStore) -> ShapeRecord {
        let Self {
            handle,
            local_rect,
            fit,
            min_filter,
            mag_filter,
            downsample,
            tint,
        } = self;
        ShapeRecord::Image {
            local_rect,
            tint: tint.into(),
            source: ImageSource::Texture {
                id: handle.id(),
                size: handle.size(),
                generation: handle.generation(),
            },
            fit,
            min_filter,
            mag_filter,
            downsample,
        }
    }
}
