use crate::icons::icon_set::IconHandle;
use crate::primitives::color::Color;
use crate::primitives::rect::Rect;
use crate::scene::record_store::RecordStore;
use crate::scene::shapes::record::ShapeRecord;
use crate::shape::local_rect_paint_empty;
use crate::shape::sealed;

/// How a baked icon's artwork maps onto its paint rect.
///
/// Unlike [`ImageFit`](crate::ImageFit) this picks a *rasterization* box, not
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
}

impl IconShape {
    /// Paint into `rect`, in owner-relative coords, instead of the owner's
    /// whole arranged rect.
    pub fn at(mut self, rect: impl Into<Rect>) -> Self {
        self.local_rect = Some(rect.into());
        self
    }

    pub fn fit(mut self, fit: impl Into<IconFit>) -> Self {
        self.fit = fit.into();
        self
    }

    /// Multiply the icon by `tint` — whole for a tintable icon, alpha only
    /// for a colour one. See the type docs.
    pub fn tint(mut self, tint: impl Into<Color>) -> Self {
        self.tint = tint.into();
        self
    }
}

// See the `sealed` module in `shape/mod.rs` for why.
#[allow(private_interfaces)]
impl sealed::Lower for IconShape {
    fn is_noop(&self) -> bool {
        local_rect_paint_empty(&self.local_rect) || self.tint.is_noop()
    }

    fn lower(self, _store: &RecordStore) -> ShapeRecord {
        ShapeRecord::Icon {
            local_rect: self.local_rect,
            handle: self.handle,
            fit: self.fit,
            tint: self.tint.into(),
        }
    }
}
