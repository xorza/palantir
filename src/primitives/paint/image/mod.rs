//! User-supplied raster images: the pure data types. The stateful lifecycle
//! lives in [`crate::renderer::image_registry`].

pub(crate) mod error;

use crate::primitives::geometry::rect::Rect;
use crate::primitives::geometry::size::Size;
use crate::primitives::math::nan::NanCheck;
use crate::primitives::paint::color::srgba_u8::SrgbaU8;
use crate::primitives::paint::image::error::ImageDataError;
use glam::{UVec2, Vec2};

/// How an image's intrinsic size maps onto its paint rect, as CSS `object-fit`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum ImageFit {
    /// Stretch to fill the rect exactly, ignoring aspect ratio. The default.
    #[default]
    Fill,
    /// Preserve aspect ratio and fit inside the rect, letterboxing.
    Contain,
    /// Preserve aspect ratio and fill the rect, cropping the longer axis
    /// (centered).
    Cover,
    /// Paint at intrinsic pixel size, centered; a larger image overflows
    /// uncropped.
    None,
    /// Repeat across the paint rect. The UV is taken raw from `offset` and
    /// `scale` (intrinsic size ignored) and wrapped with `fract()` in the
    /// shader: `scale` is the repeat count (`uv_size`), `offset` the scroll
    /// phase (`uv_min`).
    Tile {
        /// Scroll phase, as a raw UV origin.
        offset: Vec2,
        /// Repeats across the paint rect, as a raw UV extent.
        scale: Vec2,
    },
}

/// How texels are interpolated when an image paints at a size other than its
/// intrinsic one; [`Shape::image`](crate::widget::Shape::image) picks it
/// separately for minification and magnification. Implemented as a UV
/// texel-center snap, so every combination shares one sampler and bind group.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ImageFilter {
    /// Bilinear smoothing. The default.
    #[default]
    Linear,
    /// Hard texel edges, for pixel art and checkerboards.
    Nearest,
}

/// Extra taps taken when an image minifies, and how they combine.
///
/// One bilinear tap reads a 2×2 neighbourhood however far the image shrinks,
/// and which texels it reads moves with the fractional UV, so panning makes
/// fine detail scintillate. Spreading taps across the pixel's derivative
/// footprint stops that.
///
/// Opt in via [`ImageShape::downsample`](crate::widget::ImageShape::downsample);
/// the taps cost fill rate. Magnified and 1:1 draws take the single tap.
/// Coverage is exact to 8× minification (the tap grid is capped); past that it
/// is an evenly spread sample of the footprint.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ImageDownsample {
    /// One bilinear tap. The default.
    #[default]
    Single,
    /// Average the taps: the area filter, right for photographic content.
    Mean,
    /// Keep the brightest tap by luminance, so a point source survives
    /// minification (averaging a one-texel star over a 5×5 footprint costs it
    /// 25× of its peak). Whole taps win, keeping colour, but it reads brighter
    /// than the true average.
    Peak,
}

/// A CPU pixel buffer: straight (non-premultiplied) sRGB RGBA8. The backend
/// premultiplies on upload to a `Rgba8UnormSrgb` texture, since filtering
/// straight colour across a soft edge darkens it. Window icons use the same
/// storage.
///
/// Registration stages the borrowed pixels without keeping a CPU copy; keep
/// the buffer to refill it for [`ImageHandle::update`](crate::ImageHandle::update).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Image {
    pub(crate) size: UVec2,
    pub(crate) pixels: Vec<u8>,
}

impl Image {
    /// Build from raw RGBA8 bytes.
    ///
    /// # Errors
    ///
    /// [`ImageDataError`] for a zero dimension, an unrepresentable byte
    /// length, or when `pixels.len() != size.x * size.y * 4`.
    pub fn from_srgba8(size: UVec2, pixels: Vec<u8>) -> Result<Self, ImageDataError> {
        let expected = rgba8_len(size)?;
        if pixels.len() != expected {
            return Err(ImageDataError::LengthMismatch {
                size,
                expected,
                actual: pixels.len(),
            });
        }
        Ok(Self { size, pixels })
    }

    /// Transparent black at `size`, for a surface to fill before registering
    /// and before each [`ImageHandle::update`](crate::ImageHandle::update).
    ///
    /// # Panics
    ///
    /// Panics for the size errors [`Self::from_srgba8`] returns.
    pub fn blank(size: UVec2) -> Self {
        Self {
            size,
            pixels: vec![0; rgba8_len(size).unwrap_or_else(|error| panic!("{error}"))],
        }
    }

    /// Width and height in texels.
    pub const fn size(&self) -> UVec2 {
        self.size
    }

    /// The texels, row-major: sRGB-encoded channels, straight alpha.
    pub fn texels(&self) -> &[SrgbaU8] {
        bytemuck::cast_slice(&self.pixels)
    }

    /// The texels for writing.
    pub fn texels_mut(&mut self) -> &mut [SrgbaU8] {
        bytemuck::cast_slice_mut(&mut self.pixels)
    }

    /// Every texel from its column and row, row by row.
    pub fn fill_with(&mut self, mut texel: impl FnMut(u32, u32) -> SrgbaU8) {
        let width = self.size.x as usize;
        for (row, line) in self.texels_mut().chunks_exact_mut(width).enumerate() {
            for (column, out) in line.iter_mut().enumerate() {
                *out = texel(column as u32, row as u32);
            }
        }
    }

    /// One row, `size().x` texels wide.
    ///
    /// # Panics
    ///
    /// Panics when `row` is outside the image.
    pub fn row_mut(&mut self, row: u32) -> &mut [SrgbaU8] {
        let width = self.size.x as usize;
        let start = row as usize * width;
        &mut self.texels_mut()[start..start + width]
    }

    /// Copy `row` into every other row, so a texture varying along one axis is
    /// built as one row.
    ///
    /// # Panics
    ///
    /// Panics when `row` is outside the image.
    pub fn repeat_row(&mut self, row: u32) {
        let width = self.size.x as usize;
        let rows = self.size.y as usize;
        debug_assert!(rows > row as usize, "row {row} of {rows}");
        let source = row as usize * width;
        let texels = self.texels_mut();
        for target in (0..rows).filter(|target| *target != row as usize) {
            texels.copy_within(source..source + width, target * width);
        }
    }
}

fn rgba8_len(size: UVec2) -> Result<usize, ImageDataError> {
    if size.x == 0 || size.y == 0 {
        return Err(ImageDataError::ZeroSize { size });
    }
    u64::from(size.x)
        .checked_mul(u64::from(size.y))
        .and_then(|texels| texels.checked_mul(4))
        .and_then(|len| usize::try_from(len).ok())
        .ok_or(ImageDataError::TooLarge { size })
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

impl ImageFit {
    /// Fit an image of `intrinsic` logical px into `base`. The one resolver:
    /// the encoder draws with it and the cascade bounds damage by its `rect`.
    ///
    /// An empty `intrinsic` or `base` paints `base` at full UV.
    pub(crate) const fn resolve(self, base: Rect, intrinsic: Vec2) -> FitRect {
        let (iw, ih) = (intrinsic.x, intrinsic.y);
        let (bw, bh) = (base.size.w, base.size.h);
        if iw <= 0.0 || ih <= 0.0 || bw <= 0.0 || bh <= 0.0 {
            return FitRect::whole(base);
        }
        match self {
            Self::Fill => FitRect::whole(base),
            Self::Contain => {
                let scale = (bw / iw).min(bh / ih);
                FitRect::whole(centered_in(base, iw * scale, ih * scale))
            }
            Self::Cover => {
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
            Self::None => FitRect::whole(centered_in(base, iw, ih)),
            // The shader wraps the raw UV with `fract`.
            Self::Tile { offset, scale } => FitRect {
                rect: base,
                uv_min: offset,
                uv_size: scale,
            },
        }
    }
}

/// A `w`x`h` box centred inside `base`.
const fn centered_in(base: Rect, w: f32, h: f32) -> Rect {
    Rect {
        min: Vec2::new(
            base.min.x + (base.size.w - w) * 0.5,
            base.min.y + (base.size.h - h) * 0.5,
        ),
        size: Size { w, h },
    }
}

impl NanCheck for ImageFit {
    #[inline]
    fn has_nan(&self) -> bool {
        match self {
            Self::Fill | Self::Contain | Self::Cover | Self::None => false,
            Self::Tile { offset, scale } => offset.has_nan() || scale.has_nan(),
        }
    }
}

#[cfg(test)]
mod tests;
