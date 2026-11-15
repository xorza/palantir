//! [`ImagePlacement`]: how an image lies in its paint rect, and where it then paints.

use crate::primitives::geometry::fit::Fit;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::geometry::size::Size;
use crate::primitives::math::nan::NanCheck;
use glam::Vec2;

/// How an image lies in its paint rect: fit by its intrinsic size, as CSS `object-fit`, or tiled
/// across it, as CSS `background-repeat` and Flutter's `ImageRepeat` sit beside the fit.
/// [`ImagePaint::fit`](crate::ImagePaint::fit) and
/// [`ImagePaint::tile`](crate::ImagePaint::tile) set it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum ImagePlacement {
    /// Fit by its intrinsic size; [`Fit::Natural`] centers it, as `object-fit: none` does.
    Fit(Fit),
    /// Repeat across the paint rect. The UV is taken raw from `offset` and `scale` (intrinsic size
    /// ignored) and wrapped with `fract()` in the shader: `scale` is the repeat count
    /// (`uv_size`), `offset` the scroll phase (`uv_min`).
    Tile { offset: Vec2, scale: Vec2 },
}

impl ImagePlacement {
    /// Stretched to the rect, as an image paints by default.
    pub(crate) const FILL: Self = Self::Fit(Fit::Fill);
}

/// Where a fitted image paints, and which part of the texture shows.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct FitRect {
    pub(crate) rect: Rect,
    pub(crate) uv_min: Vec2,
    pub(crate) uv_size: Vec2,
}

impl FitRect {
    const fn whole(rect: Rect) -> Self {
        Self {
            rect,
            uv_min: Vec2::ZERO,
            uv_size: Vec2::ONE,
        }
    }
}

impl ImagePlacement {
    /// Place an image of `intrinsic` logical px in `base`. The one resolver: the encoder draws
    /// with it and the cascade bounds damage by its `rect`. `Cover` crops by UV, as an image has
    /// texels to drop, where a shape's overflows.
    ///
    /// An empty `intrinsic` or `base` paints `base` at full UV.
    pub(crate) const fn resolve(self, base: Rect, intrinsic: Size) -> FitRect {
        let Size { w: iw, h: ih } = intrinsic;
        let (bw, bh) = (base.size.w, base.size.h);
        if iw <= 0.0 || ih <= 0.0 || bw <= 0.0 || bh <= 0.0 {
            return FitRect::whole(base);
        }
        match self {
            Self::Fit(Fit::Fill) => FitRect::whole(base),
            Self::Fit(Fit::Contain) => {
                let scale = (bw / iw).min(bh / ih);
                FitRect::whole(centered_in(base, intrinsic.scaled_by(scale)))
            }
            Self::Fit(Fit::Cover) => {
                // The larger axis ratio decides the scale; the UV crops the overhang.
                let scale = (bw / iw).max(bh / ih);
                let uv_w = bw / (iw * scale);
                let uv_h = bh / (ih * scale);
                FitRect {
                    rect: base,
                    uv_min: Vec2::new((1.0 - uv_w) * 0.5, (1.0 - uv_h) * 0.5),
                    uv_size: Vec2::new(uv_w, uv_h),
                }
            }
            Self::Fit(Fit::Natural) => FitRect::whole(centered_in(base, intrinsic)),
            // The shader wraps the raw UV with `fract`.
            Self::Tile { offset, scale } => FitRect {
                rect: base,
                uv_min: offset,
                uv_size: scale,
            },
        }
    }
}

/// A box of `size` centered inside `base`.
const fn centered_in(base: Rect, size: Size) -> Rect {
    Rect {
        min: Vec2::new(
            base.min.x + (base.size.w - size.w) * 0.5,
            base.min.y + (base.size.h - size.h) * 0.5,
        ),
        size,
    }
}

impl NanCheck for ImagePlacement {
    #[inline]
    fn has_nan(&self) -> bool {
        match self {
            Self::Fit(_) => false,
            Self::Tile { offset, scale } => offset.has_nan() || scale.has_nan(),
        }
    }
}
