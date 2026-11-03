//! [`IconFit`]: how a baked icon's artwork maps onto its paint rect, beside [`ImageFit`].

use crate::primitives::geometry::rect::Rect;
use crate::primitives::paint::image::ImageFit;
use glam::Vec2;

/// How a baked icon's artwork maps onto its paint rect: a rasterization box, so no `Cover` or `Tile`.
#[repr(u8)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum IconFit {
    /// Rasterize to exactly the rect, stretching if needed.
    Fill,
    /// Preserve aspect ratio and fit inside the rect, centered; the default.
    #[default]
    Contain,
    /// Rasterize at the artwork's viewBox size in logical px, centered.
    None,
}

impl IconFit {
    const fn to_image_fit(self) -> ImageFit {
        match self {
            Self::Fill => ImageFit::Fill,
            Self::Contain => ImageFit::Contain,
            Self::None => ImageFit::None,
        }
    }

    /// The rect a `view_box`-sized artwork rasterizes to in `base`; a degenerate viewBox paints `base`.
    pub(crate) const fn resolve(self, base: Rect, view_box: Vec2) -> Rect {
        self.to_image_fit().resolve(base, view_box).rect
    }
}

#[cfg(test)]
mod tests {
    use crate::primitives::geometry::rect::Rect;
    use crate::primitives::paint::image::icon_fit::IconFit;
    use glam::Vec2;

    /// Every mode is a rect. A 24x12 artwork in a 100x100 rect: `Contain` gives
    /// 100x50 centered vertically, `Fill` the rect, `None` 24x12 centered.
    #[test]
    fn icon_fit_resolves_to_hand_computed_rects() {
        let base = Rect::new(10.0, 20.0, 100.0, 100.0);
        let art = Vec2::new(24.0, 12.0);

        // scale = 100/24 → 100 x 50, dy = 25.
        let contained = IconFit::Contain.resolve(base, art);
        assert_eq!(contained.min, Vec2::new(10.0, 45.0));
        assert_eq!((contained.size.w, contained.size.h), (100.0, 50.0));

        assert_eq!(IconFit::Fill.resolve(base, art), base);

        // Intrinsic px, centered: dx = 38, dy = 44.
        let intrinsic = IconFit::None.resolve(base, art);
        assert_eq!(intrinsic.min, Vec2::new(48.0, 64.0));
        assert_eq!((intrinsic.size.w, intrinsic.size.h), (24.0, 12.0));

        let square = Rect::new(0.0, 0.0, 32.0, 32.0);
        assert_eq!(IconFit::Contain.resolve(square, Vec2::splat(16.0)), square);

        assert_eq!(IconFit::Contain.resolve(base, Vec2::ZERO), base);
    }
}
