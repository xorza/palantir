//! The baked-icon builder and the fit policy that picks its rasterization
//! box. Lowers to `ShapeRecord::Icon`.

use crate::icons::icon_set::IconHandle;
use crate::primitives::color::Color;
use crate::primitives::image::ImageFit;
use crate::primitives::rect::Rect;
use crate::scene::record_store::RecordStore;
use crate::scene::shapes::record::ShapeRecord;
use crate::shape::sealed;

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
    pub(crate) fn to_image_fit(self) -> ImageFit {
        match self {
            Self::Contain => ImageFit::Contain,
            Self::Fill => ImageFit::Fill,
            Self::None => ImageFit::None,
        }
    }
}

/// A baked SVG icon painted into the owner's rect, rasterized at the exact
/// physical pixel size it lands on.
///
/// Three knobs, all of which mean something — the sampling controls an
/// [`ImageShape`](crate::ImageShape) carries have no meaning here, because
/// nothing is ever resampled.
///
/// `tint` reads differently for the two kinds of icon, following what the
/// artwork can support: a **tintable** icon (one whose every paint is a single
/// colour) takes the tint whole, so one baked icon serves every theme colour;
/// a **colour** icon takes only the tint's alpha, so it can be faded for a
/// disabled state but not recoloured.
#[derive(Clone, Copy, Debug)]
pub struct IconShape {
    pub(crate) handle: IconHandle,
    pub(crate) local_rect: Option<Rect>,
    pub(crate) fit: IconFit,
    pub(crate) tint: Color,
    pub(crate) desaturate: bool,
}

local_rect_shape!(IconShape, at);

shape_setters!(IconShape {
    fit: IconFit => fit,
    /// Multiply the icon by `tint` — whole for a tintable icon, alpha only
    /// for a colour one. See the type docs.
    tint: Color => tint,
});

impl IconShape {
    /// Draw a **colour** icon in greyscale — its own luminance, hue gone.
    ///
    /// The disabled look for artwork whose colours a tint cannot replace.
    /// Pairs with a faded `tint` alpha, which is the other half of the same
    /// state. No effect on a tintable icon: there the draw already picks the
    /// colour, so a grey one is a grey `tint`.
    pub fn desaturate(mut self, desaturate: bool) -> Self {
        self.desaturate = desaturate;
        self
    }
}

impl sealed::LowerShape for IconShape {
    fn is_noop(&self) -> bool {
        self.rect_is_noop() || self.tint.is_noop()
    }

    fn lower(self, _store: &RecordStore) -> ShapeRecord {
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
