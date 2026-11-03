//! The baked-icon builder and its fit policy; lowers to `ShapeRecord::Icon`.

use crate::primitives::geometry::rect::Rect;
use crate::primitives::identity::icon_handle::IconHandle;
use crate::primitives::math::domain;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::image::icon_fit::IconFit;
use crate::shape::record::ShapeRecord;
use crate::shape::record_store::RecordStore;
use crate::shape::sealed;

/// A baked SVG icon painted into the owner's rect, rasterized at its exact
/// physical size. `tint` applies whole to a **tintable** icon (single-color
/// paints) but only as alpha to a **color** icon.
#[derive(Clone, Copy, Debug)]
#[must_use]
pub struct IconShape {
    pub(crate) handle: IconHandle,
    pub(crate) local_rect: Option<Rect>,
    pub(crate) fit: IconFit,
    pub(crate) tint: RgbaF32,
    pub(crate) desaturate: bool,
}

impl IconShape {
    pub(super) fn new(handle: IconHandle) -> Self {
        Self {
            handle,
            local_rect: None,
            fit: IconFit::default(),
            tint: RgbaF32::WHITE,
            desaturate: false,
        }
    }

    /// Paints into `rect`, in owner-relative coords, instead of the owner's arranged rect.
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

    /// How the glyph is placed inside its box.
    pub const fn fit(mut self, fit: IconFit) -> Self {
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

impl sealed::LowerShape for IconShape {
    fn is_noop(&self) -> bool {
        self.local_rect.is_some_and(Rect::is_paint_empty) || self.tint.is_noop()
    }

    fn lower(self, _store: &mut RecordStore) -> ShapeRecord {
        let Self {
            handle,
            local_rect,
            fit,
            tint,
            desaturate,
        } = self;
        ShapeRecord::Icon {
            local_rect,
            handle,
            fit,
            tint: tint.to_f16(),
            desaturate,
        }
    }
}
