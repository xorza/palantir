use crate::primitives::color::Color;
use crate::primitives::image::{ImageDownsample, ImageFilter, ImageFit};
use crate::primitives::rect::Rect;
use crate::renderer::image_registry::ImageHandle;
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
    pub(crate) tint: Color,
}

local_rect_shape!(ImageShape);

shape_setters!(ImageShape {
    fit: ImageFit => fit,
    min_filter: ImageFilter => min_filter,
    mag_filter: ImageFilter => mag_filter,
    /// Take extra taps where this image minifies, instead of the sampler's
    /// lone bilinear one — see [`ImageDownsample`] for what that buys and
    /// what it costs. Off by default; only worth setting on an image that
    /// actually shrinks, and that has detail fine enough to alias.
    downsample: ImageDownsample => downsample,
    tint: Color => tint,
});

impl sealed::LowerShape for ImageShape {
    fn is_noop(&self) -> bool {
        self.rect_is_noop() || self.tint.is_noop()
    }

    fn lower(self, _store: &RecordStore) -> ShapeRecord {
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
            // Extract the cheap id + size; the owning `ImageHandle` the
            // caller holds is what keeps the GPU texture alive.
            source: ImageSource::Texture {
                id: handle.id(),
                size: handle.size(),
            },
            fit,
            min_filter,
            mag_filter,
            downsample,
        }
    }
}
