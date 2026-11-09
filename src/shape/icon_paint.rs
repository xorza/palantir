//! [`IconPaint`]: a baked icon over a rect. Lowers to `ShapeRecord::Icon`.

use crate::primitives::geometry::fit::Fit;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::identity::icon_handle::IconHandle;
use crate::primitives::math::domain;
use crate::primitives::paint::color::RgbaF32;
use crate::shape::record::ShapeRecord;
use crate::shape::record_store::RecordStore;
use crate::shape::rect::RectShape;
use crate::shape::sealed::{self, Resolve};

/// A baked SVG icon painted over a [`RectShape`] ([`Ui::add_shape`](crate::Ui::add_shape)), rasterized at its exact
/// physical size. `tint` applies whole to a **tintable** icon (single-color
/// paints) but only as alpha to a **color** icon.
#[derive(Clone, Copy, Debug)]
#[must_use]
pub struct IconPaint {
    pub(crate) handle: IconHandle,
    pub(crate) fit: Fit,
    pub(crate) tint: RgbaF32,
    pub(crate) desaturate: bool,
}

impl IconPaint {
    /// The icon `handle`, fit by [`Fit::Contain`], untinted, in its own colors.
    pub const fn new(handle: IconHandle) -> Self {
        Self {
            handle,
            fit: Fit::Contain,
            tint: RgbaF32::WHITE,
            desaturate: false,
        }
    }

    /// How the glyph is placed inside its box.
    pub const fn fit(mut self, fit: Fit) -> Self {
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

/// An icon over a rect, square-cornered and covering its inside.
impl sealed::LowerShape for (RectShape, IconPaint) {
    /// # Panics
    ///
    /// Unless the rect is square-cornered and covers its inside, which an icon's pipeline draws.
    #[track_caller]
    fn validate(&self) {
        self.0.assert_plain("an icon");
    }

    fn lower(self, _resolve: Resolve<'_>, _store: &mut RecordStore) -> Option<ShapeRecord> {
        let (
            RectShape {
                local_rect,
                corners: _,
                coverage: _,
            },
            IconPaint {
                handle,
                fit,
                tint,
                desaturate,
            },
        ) = self;
        if local_rect.is_some_and(Rect::is_paint_empty) || tint.is_noop() {
            return None;
        }
        Some(ShapeRecord::Icon {
            local_rect,
            handle,
            fit,
            tint: tint.to_f16(),
            desaturate,
        })
    }
}
