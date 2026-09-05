//! The textured-rectangle builder. Lowers to `ShapeRecord::Image`.

use crate::primitives::color::RgbaF32;
use crate::primitives::image::{ImageDownsample, ImageFilter, ImageFit};
use crate::primitives::nan::NanCheck;
use crate::primitives::rect::Rect;
use crate::renderer::image_registry::image_handle::ImageHandle;
use crate::scene::record_store::RecordStore;
use crate::scene::shapes::paint::ImageSource;
use crate::scene::shapes::record::ShapeRecord;
use crate::shape::sealed;

/// Textured rectangle painted from a registered [`ImageHandle`].
#[derive(Clone, Debug)]
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
    /// Paint into `rect`, in owner-relative coords, instead of the
    /// owner's whole arranged rect.
    pub fn at(mut self, rect: impl Into<Rect>) -> Self {
        self.local_rect = Some(rect.into());
        self
    }

    pub fn fit(mut self, fit: impl Into<ImageFit>) -> Self {
        self.fit = fit.into();
        self
    }

    pub fn min_filter(mut self, min_filter: impl Into<ImageFilter>) -> Self {
        self.min_filter = min_filter.into();
        self
    }

    pub fn mag_filter(mut self, mag_filter: impl Into<ImageFilter>) -> Self {
        self.mag_filter = mag_filter.into();
        self
    }

    /// Take extra taps where this image minifies, instead of the sampler's
    /// lone bilinear one — see [`ImageDownsample`] for what that buys and
    /// what it costs. Off by default; only worth setting on an image that
    /// actually shrinks, and that has detail fine enough to alias.
    pub fn downsample(mut self, downsample: impl Into<ImageDownsample>) -> Self {
        self.downsample = downsample.into();
        self
    }

    pub fn tint(mut self, tint: impl Into<RgbaF32>) -> Self {
        self.tint = tint.into();
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
