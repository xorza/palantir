//! The baked-icon builder and its fit policy; lowers to `ShapeRecord::Icon`.

use crate::icons::icon_set::IconHandle;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::math::domain;
use crate::primitives::math::nan::NanCheck;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::image::ImageFit;
use crate::scene::record_store::RecordStore;
use crate::shape::record::ShapeRecord;
use crate::shape::sealed;
use glam::Vec2;

/// How a baked icon's artwork maps onto its paint rect: a rasterization box, so no `Cover` or `Tile`.
#[repr(u8)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum IconFit {
    /// Preserve aspect ratio and fit inside the rect, centered; the default.
    #[default]
    Contain,
    /// Rasterize to exactly the rect, stretching if needed.
    Fill,
    /// Rasterize at the artwork's viewBox size in logical px, centered.
    None,
}

impl IconFit {
    const fn to_image_fit(self) -> ImageFit {
        match self {
            Self::Contain => ImageFit::Contain,
            Self::Fill => ImageFit::Fill,
            Self::None => ImageFit::None,
        }
    }

    /// The rect a `view_box`-sized artwork rasterizes to in `base`; a degenerate viewBox paints `base`.
    pub(crate) const fn resolve(self, base: Rect, view_box: Vec2) -> Rect {
        self.to_image_fit().resolve(base, view_box).rect
    }
}

/// A baked SVG icon painted into the owner's rect, rasterized at its exact
/// physical size. `tint` applies whole to a **tintable** icon (single-colour
/// paints) but only as alpha to a **colour** icon.
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
}

impl IconShape {
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
    /// Panics unless `tint` is a [colour](crate::widget::domain::color).
    #[track_caller]
    pub const fn tint(mut self, tint: RgbaF32) -> Self {
        self.tint = domain::color(tint);
        self
    }

    /// Draws a **colour** icon in greyscale; no effect on a tintable icon.
    pub const fn desaturate(mut self, desaturate: bool) -> Self {
        self.desaturate = desaturate;
        self
    }
}

impl sealed::LowerShape for IconShape {
    fn is_noop(&self) -> bool {
        self.local_rect.is_some_and(Rect::is_paint_empty) || self.tint.is_noop()
    }

    fn has_nan(&self) -> bool {
        self.local_rect.has_nan() || self.tint.has_nan()
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
            tint: tint.into(),
            desaturate,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::primitives::geometry::rect::Rect;
    use crate::shape::icon::IconFit;
    use glam::Vec2;

    /// Every mode is a rect. A 24x12 artwork in a 100x100 rect: `Contain` gives
    /// 100x50 centred vertically, `Fill` the rect, `None` 24x12 centred.
    #[test]
    fn icon_fit_resolves_to_hand_computed_rects() {
        let base = Rect::new(10.0, 20.0, 100.0, 100.0);
        let art = Vec2::new(24.0, 12.0);

        // scale = 100/24 → 100 x 50, dy = 25.
        let contained = IconFit::Contain.resolve(base, art);
        assert_eq!(contained.min, Vec2::new(10.0, 45.0));
        assert_eq!((contained.size.w, contained.size.h), (100.0, 50.0));

        assert_eq!(IconFit::Fill.resolve(base, art), base);

        // Intrinsic px, centred: dx = 38, dy = 44.
        let intrinsic = IconFit::None.resolve(base, art);
        assert_eq!(intrinsic.min, Vec2::new(48.0, 64.0));
        assert_eq!((intrinsic.size.w, intrinsic.size.h), (24.0, 12.0));

        let square = Rect::new(0.0, 0.0, 32.0, 32.0);
        assert_eq!(IconFit::Contain.resolve(square, Vec2::splat(16.0)), square);

        assert_eq!(IconFit::Contain.resolve(base, Vec2::ZERO), base);
    }
}
