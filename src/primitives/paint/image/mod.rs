//! User-supplied raster images — the pure data types.
//!
//! [`Image`] is a decoded pixel buffer and [`ImageFit`] is the
//! intrinsic-size-to-rect mapping. The stateful lifecycle (registration,
//! GPU upload/release, the RAII `ImageHandle`, the `TextureId` identity)
//! lives in [`crate::renderer::image_registry`] — `primitives` stays a
//! pure leaf.

use crate::primitives::geometry::rect::Rect;
use crate::primitives::geometry::size::Size;
use crate::primitives::math::nan::NanCheck;
use crate::primitives::paint::color::srgba_u8::SrgbaU8;
use glam::{UVec2, Vec2};

/// How an image's intrinsic size maps onto its paint rect. Same
/// semantics as CSS `object-fit`. `Fill` (the default) stretches the
/// image to exactly fill the rect — fastest, no UV crop needed.
/// `Contain` / `None` produce a smaller paint rect inside the owner;
/// `Cover` produces a UV crop so the full rect is painted with the
/// image's centered portion. `Tile` repeats the image across the rect.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum ImageFit {
    /// Stretch the image to fill the rect exactly. Aspect ratio not
    /// preserved. The default.
    #[default]
    Fill,
    /// Preserve aspect ratio; fit the image entirely inside the rect.
    /// Letterboxes (transparent margins) if aspect ratios differ.
    Contain,
    /// Preserve aspect ratio; fill the rect entirely. Crops the
    /// image's longer axis (centered).
    Cover,
    /// Paint at the image's intrinsic pixel size, centered in the rect.
    /// An image larger than the rect overflows it, uncropped.
    None,
    /// Repeat the image across the paint rect. The UV is taken raw from
    /// `offset`/`scale` (intrinsic image size ignored) and wrapped with
    /// `fract()` in the shader: `scale` is the number of repeats across
    /// the rect (`uv_size`), `offset` the scroll phase (`uv_min`). The
    /// caller drives both — e.g. a pannable/zoomable dotted backdrop
    /// sets `scale = viewport / tile_px`, `offset = -pan / tile_px`.
    Tile {
        /// Scroll phase, as a raw UV origin.
        offset: Vec2,
        /// Repeats across the paint rect, as a raw UV extent.
        scale: Vec2,
    },
}

/// How texels are interpolated when an image paints at a size other
/// than its intrinsic one. `Linear` (the default) is bilinear
/// smoothing; `Nearest` keeps hard texel edges — pixel-art upscales,
/// checkerboards, pixel peeping. [`Shape::image`](crate::widget::Shape::image) chooses this
/// independently for minification and magnification. Implemented as a
/// UV texel-center snap in the image shader, so every combination
/// shares one sampler and one bind group per texture. Serde (lowercase)
/// lets hosts persist a filter choice in their config files.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ImageFilter {
    /// Bilinear smoothing. The default.
    #[default]
    Linear,
    /// Hard texel edges, for pixel art and checkerboards.
    Nearest,
}

/// Extra taps taken when an image *minifies*, and how they combine.
///
/// One bilinear tap reads a 2×2 texel neighbourhood however far the image is
/// shrunk, so at 5× minification about 4 of each pixel's ~27 source texels
/// reach the screen — and *which* 4 moves with the fractional UV, so panning
/// makes fine detail scintillate: a starfield, a wire grid, a downscaled
/// screenshot's text. Spreading taps across the pixel's derivative footprint
/// reads enough of it for that to stop.
///
/// Opt in per shape via [`ImageShape::downsample`](crate::widget::ImageShape::downsample).
/// The taps cost fill rate on every fragment the image minifies into, which is
/// why they are not the default — a UI icon or a 1:1 blit should not pay for
/// them. Magnified and 1:1 draws take the single tap whatever this says, since
/// there is no footprint left to cover.
///
/// Coverage is exact to 8× minification (the tap grid is capped, and each
/// bilinear tap spans 2 texels); past that it is a bounded, evenly spread
/// sample of the footprint rather than the whole of it.
///
/// No serde, unlike [`ImageFilter`]: nothing persists this yet, and the derive
/// can arrive with the first host that puts it in a config file.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ImageDownsample {
    /// One bilinear tap — what the sampler does on its own. The default, and
    /// exactly right whenever the image is not being shrunk.
    #[default]
    Single,
    /// Average the taps: the area filter, and the honest answer for
    /// photographic content — what a correct downscale of that region looks
    /// like.
    Mean,
    /// Keep the brightest tap, by luminance, so a point source survives a
    /// footprint it occupies a fraction of. Averaging a one-texel star across a
    /// 5×5 footprint costs it 25× of its peak, and a starfield zoomed out
    /// reads as empty; this keeps the star — and its colour, since a whole tap
    /// wins rather than each channel separately — at the cost of sitting
    /// brighter than the true area average.
    Peak,
}

/// A CPU pixel buffer. Straight (non-premultiplied) sRGB RGBA8 — the
/// backend uploads it into a `Rgba8UnormSrgb` texture, scaling each
/// colour by its own alpha on the way, because the sampler filters what
/// the texture holds and straight colour filtered across a soft edge
/// darkens it. Window icons use the same validated storage.
///
/// Registration stages the borrowed pixels without retaining a CPU copy.
/// Keep the buffer to refill it for [`ImageHandle::update`](crate::ImageHandle::update).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Image {
    pub(crate) size: UVec2,
    pub(crate) pixels: Vec<u8>,
}

impl Image {
    /// Build from raw RGBA8 bytes.
    ///
    /// # Panics
    ///
    /// Panics for a zero dimension, an unrepresentable byte length, or when
    /// `pixels.len() != size.x * size.y * 4`.
    pub fn from_srgba8(size: UVec2, pixels: Vec<u8>) -> Self {
        let expected = rgba8_len(size.x, size.y);
        assert_eq!(
            pixels.len(),
            expected,
            "RGBA8 byte length {} does not match {}x{}x4 = {expected}",
            pixels.len(),
            size.x,
            size.y,
        );
        Self { size, pixels }
    }

    /// Transparent black at `size`: what a surface fills before it registers,
    /// and refills before each [`ImageHandle::update`](crate::ImageHandle::update).
    ///
    /// # Panics
    ///
    /// Panics for a zero dimension or an unrepresentable byte length, as
    /// [`Self::from_srgba8`] does.
    pub fn blank(size: UVec2) -> Self {
        Self {
            size,
            pixels: vec![0; rgba8_len(size.x, size.y)],
        }
    }

    /// Width and height in texels.
    pub fn size(&self) -> UVec2 {
        self.size
    }

    /// The texels, row-major: sRGB-encoded channels and a straight alpha
    /// — what an application draws with, and what the upload converts.
    pub fn texels(&self) -> &[SrgbaU8] {
        bytemuck::cast_slice(&self.pixels)
    }

    /// The texels for writing. The size is fixed, so the slice is too.
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

    /// Copy `row` into every other row. A texture that varies along one
    /// axis only is built as one row and repeated, which keeps a bar's
    /// rebuild at one conversion per column.
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

fn rgba8_len(width: u32, height: u32) -> usize {
    assert!(
        width != 0 && height != 0,
        "RGBA8 dimensions must be non-zero, got {width}x{height}",
    );
    u64::from(width)
        .checked_mul(u64::from(height))
        .and_then(|texels| texels.checked_mul(4))
        .and_then(|len| usize::try_from(len).ok())
        .expect("RGBA8 dimensions overflow addressable byte length")
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
    /// Fit an image of `intrinsic` logical px into `base`. The one
    /// resolver: the encoder draws with the answer and the cascade bounds
    /// the shape's damage by its `rect`, so the two cannot disagree about
    /// where an overflowing image paints.
    ///
    /// An empty `intrinsic` or `base` — an image with no registry entry,
    /// an app GPU view — paints `base` at full UV.
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
                // The larger axis ratio decides the scale, so the image
                // overhangs `base`; the UV crops the overhang, centred.
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
            // The shader wraps the raw UV with `fract`; `scale` and
            // `offset` already say the repeat count and phase.
            Self::Tile { offset, scale } => FitRect {
                rect: base,
                uv_min: offset,
                uv_size: scale,
            },
        }
    }
}

/// A `w`x`h` box centred inside `base` — where every aspect-preserving
/// fit puts the leftover space.
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
