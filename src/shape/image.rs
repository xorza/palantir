use crate::primitives::color::Color;
use crate::primitives::image::{ImageFilter, ImageFit};
use crate::primitives::rect::Rect;
use crate::renderer::image_registry::ImageHandle;
use crate::shape::local_rect_paint_empty;

/// Textured rectangle painted from a registered [`ImageHandle`].
#[derive(Clone, Debug)]
pub struct ImageShape {
    pub(crate) handle: ImageHandle,
    pub(crate) local_rect: Option<Rect>,
    pub(crate) fit: ImageFit,
    pub(crate) min_filter: ImageFilter,
    pub(crate) mag_filter: ImageFilter,
    pub(crate) tint: Color,
}

impl ImageShape {
    pub fn at(mut self, rect: impl Into<Rect>) -> Self {
        self.local_rect = Some(rect.into());
        self
    }

    pub fn fit(mut self, fit: impl Into<ImageFit>) -> Self {
        self.fit = fit.into();
        self
    }

    pub fn min_filter(mut self, filter: impl Into<ImageFilter>) -> Self {
        self.min_filter = filter.into();
        self
    }

    pub fn mag_filter(mut self, filter: impl Into<ImageFilter>) -> Self {
        self.mag_filter = filter.into();
        self
    }

    pub fn tint(mut self, tint: impl Into<Color>) -> Self {
        self.tint = tint.into();
        self
    }

    pub(crate) fn is_noop(&self) -> bool {
        local_rect_paint_empty(&self.local_rect) || self.tint.is_noop()
    }
}
