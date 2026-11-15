//! [`Fit`]: how geometry of its own extent fills a rect, and the [`FitMap`] it resolves to.

use crate::primitives::geometry::rect::Rect;
use crate::primitives::geometry::size::Size;
use crate::primitives::math::domain::is_invisible;
use glam::Vec2;

/// How content of its own extent fills a rect: an image's texels, an icon's artwork or a point
/// shape's points, with CSS `object-fit`'s names, as WPF's `Stretch` scales a `Polygon`, a `Path`
/// and an `Image` alike. A stroke's width and a triangle's corner radius do not scale, as WPF's
/// stroke thickness does not. Each consumer keeps its own default: [`Self::Fill`] for an image,
/// [`Self::Natural`] for a point shape. Refused: a second enum for images, whose `Natural`
/// placed differently under the same name.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Fit {
    /// Unscaled. Content with a size of its own, an image or an icon, centers in the rect, as
    /// CSS `object-fit: none` and Flutter's `BoxFit.none` center it; a point shape's points stay as
    /// given, as WPF's `Stretch.None` leaves a `Polygon`. Named for what it keeps, as `None` would
    /// shadow `Option::None`.
    Natural = 0,
    /// Each axis scaled to fill the rect exactly.
    Fill = 1,
    /// Scaled uniformly to fit inside the rect, centered.
    Contain = 2,
    /// Scaled uniformly to cover the rect, centered. An image crops to the rect by UV, as it has
    /// texels to drop; a shape's or an icon's overflow paints past it, and a clipping ancestor crops
    /// it. Refused: a crop of their own, which no stroke or SDF draws.
    Cover = 3,
}

/// An affine map that scales each axis and then translates: `p ↦ offset + p · scale`. The scale
/// is positive, so the map keeps a rect a rect and a curve's control points its curve's.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct FitMap {
    pub(crate) offset: Vec2,
    pub(crate) scale: Vec2,
}

impl FitMap {
    /// The map that moves nothing.
    pub(crate) const IDENTITY: Self = Self {
        offset: Vec2::ZERO,
        scale: Vec2::ONE,
    };

    /// `p` mapped.
    #[inline]
    pub(crate) fn apply(self, p: Vec2) -> Vec2 {
        self.offset + p * self.scale
    }

    /// `rect` mapped.
    #[inline]
    pub(crate) fn apply_rect(self, rect: Rect) -> Rect {
        let size = Vec2::new(rect.size.w, rect.size.h) * self.scale;
        let min = self.apply(rect.min);
        Rect::new(min.x, min.y, size.x, size.y)
    }
}

/// The identity, as a payload literal's `..Default::default()` reads it.
impl Default for FitMap {
    fn default() -> Self {
        Self::IDENTITY
    }
}

impl Fit {
    /// The map that fits geometry spanning `extent` into `rect`. An axis of no extent takes no
    /// part in the scale: under [`Self::Fill`] it keeps scale 1, and it centers in the rect, as a
    /// horizontal line fills a rect's width at its middle height.
    pub(crate) fn map(self, extent: Rect, rect: Rect) -> FitMap {
        let ratio = |rect_len: f32, extent_len: f32| {
            (!is_invisible(extent_len)).then(|| rect_len / extent_len)
        };
        let sx = ratio(rect.size.w, extent.size.w);
        let sy = ratio(rect.size.h, extent.size.h);
        let uniform = |pick: fn(f32, f32) -> f32| match (sx, sy) {
            (Some(x), Some(y)) => pick(x, y),
            (Some(s), None) | (None, Some(s)) => s,
            (None, None) => 1.0,
        };
        let centered = |scale: Vec2| rect.center() - extent.center() * scale;
        match self {
            Self::Natural => FitMap {
                offset: rect.min,
                scale: Vec2::ONE,
            },
            // An axis of extent maps its ends onto the rect's, exact at the near edge, where a
            // map about the centers would round it off.
            Self::Fill => {
                let scale = Vec2::new(sx.unwrap_or(1.0), sy.unwrap_or(1.0));
                let center = centered(scale);
                let edge = rect.min - extent.min * scale;
                FitMap {
                    offset: Vec2::new(
                        if sx.is_some() { edge.x } else { center.x },
                        if sy.is_some() { edge.y } else { center.y },
                    ),
                    scale,
                }
            }
            Self::Contain => {
                let scale = Vec2::splat(uniform(f32::min));
                FitMap {
                    offset: centered(scale),
                    scale,
                }
            }
            Self::Cover => {
                let scale = Vec2::splat(uniform(f32::max));
                FitMap {
                    offset: centered(scale),
                    scale,
                }
            }
        }
    }

    /// The rect artwork of `size` rasterizes to in `rect`: [`Self::Natural`] centers it, as an
    /// image's does, and the scaling fits place it as [`Self::map`] places points.
    pub(crate) fn place_artwork(self, rect: Rect, size: Size) -> Rect {
        let extent = Rect {
            min: Vec2::ZERO,
            size,
        };
        let map = match self {
            Self::Natural => FitMap {
                offset: rect.center() - extent.center(),
                scale: Vec2::ONE,
            },
            Self::Fill | Self::Contain | Self::Cover => self.map(extent, rect),
        };
        map.apply_rect(extent)
    }
}

#[cfg(test)]
mod tests;
