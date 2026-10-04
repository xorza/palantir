//! The baked-icon builder and the fit policy that picks its rasterization
//! box. Lowers to `ShapeRecord::Icon`.

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

/// How a baked icon's artwork maps onto its paint rect.
///
/// Unlike [`ImageFit`] this picks a *rasterization* box, not
/// a UV crop: the icon is drawn at whatever size this resolves to, so there is
/// no `Cover` and no `Tile` — cropping or repeating a vector would mean
/// rasterizing something other than the icon.
#[repr(u8)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum IconFit {
    /// Preserve the artwork's aspect ratio and fit it inside the rect,
    /// centered. The default, and what a square icon in a square node gets
    /// either way.
    #[default]
    Contain,
    /// Rasterize to exactly the rect, stretching if the aspect ratios differ.
    Fill,
    /// Rasterize at the artwork's own viewBox size in logical px, centered.
    /// Overflows a smaller rect.
    None,
}

impl IconFit {
    /// The [`ImageFit`] this means, so the variants the two share
    /// resolve through the image path's one implementation rather than
    /// a second copy of it. The subset stays a subset — that is what
    /// keeps `Cover` and `Tile` unrepresentable for an icon.
    const fn to_image_fit(self) -> ImageFit {
        match self {
            Self::Contain => ImageFit::Contain,
            Self::Fill => ImageFit::Fill,
            Self::None => ImageFit::None,
        }
    }

    /// The rect an icon with a `view_box`-sized artwork rasterizes to in
    /// `base`. Only the rect: an icon rasterizes to its box, so there is
    /// no UV to crop. A degenerate viewBox paints `base`.
    pub(crate) const fn resolve(self, base: Rect, view_box: Vec2) -> Rect {
        self.to_image_fit().resolve(base, view_box).rect
    }
}

/// A baked SVG icon painted into the owner's rect, rasterized at the exact
/// physical pixel size it lands on.
///
/// Three knobs, all of which mean something — the sampling controls an
/// [`ImageShape`](crate::widget::ImageShape) carries have no meaning here, because
/// nothing is ever resampled.
///
/// `tint` reads differently for the two kinds of icon, following what the
/// artwork can support: a **tintable** icon (one whose every paint is a single
/// colour) takes the tint whole, so one baked icon serves every theme colour;
/// a **colour** icon takes only the tint's alpha, so it can be faded for a
/// disabled state but not recoloured.
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
    /// Paint into `rect`, in owner-relative coords, instead of the
    /// owner's whole arranged rect.
    pub fn at(mut self, rect: impl Into<Rect>) -> Self {
        self.local_rect = Some(rect.into());
        self
    }

    /// How the glyph is placed inside its box.
    pub fn fit(mut self, fit: impl Into<IconFit>) -> Self {
        self.fit = fit.into();
        self
    }

    /// Multiply the icon by `tint` — whole for a tintable icon, alpha only
    /// for a colour one. See the type docs.
    ///
    /// # Panics
    ///
    /// Panics unless `tint` is a [colour](crate::widget::domain::color).
    #[track_caller]
    pub fn tint(mut self, tint: impl Into<RgbaF32>) -> Self {
        self.tint = domain::color(tint.into());
        self
    }

    /// Draw a **colour** icon in greyscale — its own luminance, hue gone.
    ///
    /// The disabled look for artwork whose colours a tint cannot replace.
    /// Pairs with a faded `tint` alpha, which is the other half of the same
    /// state. No effect on a tintable icon: there the draw already picks the
    /// colour, so a grey one is a grey `tint`.
    pub fn desaturate(mut self, desaturate: impl Into<bool>) -> Self {
        self.desaturate = desaturate.into();
        self
    }
}

impl sealed::LowerShape for IconShape {
    fn is_noop(&self) -> bool {
        self.local_rect.is_some_and(Rect::is_paint_empty) || self.tint.is_noop()
    }

    /// `fit` is a bare tag, `desaturate` a flag, and the artwork's own
    /// box comes from baked data, so the rect and the tint are the whole
    /// surface a caller can put a NaN into.
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

    /// `IconFit` picks a rasterization box, so every mode is a rect and the
    /// numbers are hand-checkable. A 24x12 artwork in a 100x100 rect:
    /// `Contain` scales by min(100/24, 100/12) = 4.166.., giving 100x50 centred
    /// vertically; `Fill` takes the rect whole; `None` paints 24x12 centred.
    #[test]
    fn icon_fit_resolves_to_hand_computed_rects() {
        let base = Rect::new(10.0, 20.0, 100.0, 100.0);
        let art = Vec2::new(24.0, 12.0);

        // scale = 100/24 = 4.1666667 → 100 x 50, dy = (100 - 50)/2 = 25.
        let contained = IconFit::Contain.resolve(base, art);
        assert_eq!(contained.min, Vec2::new(10.0, 45.0));
        assert_eq!((contained.size.w, contained.size.h), (100.0, 50.0));

        assert_eq!(IconFit::Fill.resolve(base, art), base);

        // Intrinsic px, centred: dx = (100-24)/2 = 38, dy = (100-12)/2 = 44.
        let intrinsic = IconFit::None.resolve(base, art);
        assert_eq!(intrinsic.min, Vec2::new(48.0, 64.0));
        assert_eq!((intrinsic.size.w, intrinsic.size.h), (24.0, 12.0));

        // A square artwork in a square rect is the same rect under every mode
        // that preserves aspect — the case that would hide an axis mix-up.
        let square = Rect::new(0.0, 0.0, 32.0, 32.0);
        assert_eq!(IconFit::Contain.resolve(square, Vec2::splat(16.0)), square);

        // A degenerate viewBox falls through to the base rect rather than
        // dividing by zero — the same fail-safe the image path takes.
        assert_eq!(IconFit::Contain.resolve(base, Vec2::ZERO), base);
    }
}
