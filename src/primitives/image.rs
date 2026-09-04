//! User-supplied raster images — the pure data types.
//!
//! [`Image`] is a decoded pixel buffer and [`ImageFit`] is the
//! intrinsic-size-to-rect mapping. The stateful lifecycle (registration,
//! GPU upload/release, the RAII `ImageHandle`, the `TextureId` identity)
//! lives in [`crate::renderer::image_registry`] — `primitives` stays a
//! pure leaf.

use crate::primitives::color::srgba_u8::SrgbaU8;
use crate::primitives::nan::NanCheck;
use glam::UVec2;

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
        offset: glam::Vec2,
        scale: glam::Vec2,
    },
}

/// How texels are interpolated when an image paints at a size other
/// than its intrinsic one. `Linear` (the default) is bilinear
/// smoothing; `Nearest` keeps hard texel edges — pixel-art upscales,
/// checkerboards, pixel peeping. [`Shape::image`](crate::Shape::image) chooses this
/// independently for minification and magnification. Implemented as a
/// UV texel-center snap in the image shader, so every combination
/// shares one sampler and one bind group per texture. Serde (lowercase)
/// lets hosts persist a filter choice in their config files.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ImageFilter {
    #[default]
    Linear,
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
/// Opt in per shape via [`ImageShape::downsample`](crate::ImageShape::downsample).
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

/// A CPU pixel buffer. Straight (non-premultiplied) sRGB RGBA8 — the backend
/// uses a `Rgba8UnormSrgb` texture so the sampler decodes to linear on read,
/// and the shader premultiplies. Window icons use the same validated storage.
///
/// Registering one copies it to the GPU and keeps nothing of it, so the
/// caller holds exactly the copies it wants: none for a static image, one to
/// refill and hand to [`ImageHandle::update`](crate::ImageHandle::update)
/// for a surface that changes on screen. The texel helpers below are for
/// that refill.
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
    /// Panics for zero dimensions, unrepresentable byte lengths, or when
    /// `pixels.len() != width * height * 4`.
    pub fn from_rgba8(width: u32, height: u32, pixels: Vec<u8>) -> Self {
        let expected = rgba8_len(width, height);
        assert_eq!(
            pixels.len(),
            expected,
            "RGBA8 byte length {} does not match {width}x{height}x4 = {expected}",
            pixels.len(),
        );
        Self {
            size: UVec2::new(width, height),
            pixels,
        }
    }

    /// Transparent black at `size`: what a surface fills before it registers,
    /// and refills before each [`ImageHandle::update`](crate::ImageHandle::update).
    ///
    /// # Panics
    ///
    /// Panics for a zero dimension or an unrepresentable byte length, as
    /// [`Self::from_rgba8`] does.
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

    /// The texels, row-major: sRGB-encoded channels and a straight alpha,
    /// which is what an `Rgba8UnormSrgb` texture decodes on sample.
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
        assert!(rows > row as usize, "row {row} of {rows}");
        let source = row as usize * width;
        let texels = self.texels_mut();
        for target in (0..rows).filter(|target| *target != row as usize) {
            texels.copy_within(source..source + width, target * width);
        }
    }
}

/// Bytes an RGBA8 image of `width` by `height` holds.
///
/// # Panics
///
/// Panics for a zero dimension or a length `usize` cannot hold.
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

/// Only [`ImageFit::Tile`] carries scalars; every other fit is a bare tag.
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
mod tests {
    use crate::primitives::color::srgba_u8::SrgbaU8;
    use crate::primitives::image::Image;
    use glam::UVec2;

    #[test]
    fn image_stores_valid_rgba8_dimensions_and_pixels() {
        let pixels = vec![255, 0, 0, 255, 0, 255, 0, 128];
        let image = Image::from_rgba8(2, 1, pixels.clone());
        assert_eq!(image.size(), UVec2::new(2, 1));
        assert_eq!(image.pixels, pixels);

        let blank = Image::blank(UVec2::new(2, 3));
        assert_eq!(blank.size(), UVec2::new(2, 3));
        assert_eq!(blank.pixels, vec![0; 24]);
    }

    #[test]
    fn fill_with_visits_every_texel_in_row_major_order() {
        let mut image = Image::blank(UVec2::new(3, 2));
        image.fill_with(|column, row| SrgbaU8::rgb(column as u8, row as u8, 0));
        let got: Vec<(u8, u8)> = image.texels().iter().map(|t| (t.r, t.g)).collect();
        assert_eq!(got, vec![(0, 0), (1, 0), (2, 0), (0, 1), (1, 1), (2, 1)]);
    }

    #[test]
    fn repeat_row_copies_one_row_into_the_others() {
        let mut image = Image::blank(UVec2::new(2, 3));
        image
            .row_mut(1)
            .copy_from_slice(&[SrgbaU8::rgb(1, 0, 0), SrgbaU8::rgb(2, 0, 0)]);
        image.repeat_row(1);
        let reds: Vec<u8> = image.texels().iter().map(|t| t.r).collect();
        assert_eq!(reds, vec![1, 2, 1, 2, 1, 2]);
    }

    #[test]
    #[should_panic(expected = "row 3 of 3")]
    fn repeat_row_rejects_a_row_outside_the_image() {
        Image::blank(UVec2::new(2, 3)).repeat_row(3);
    }

    #[test]
    fn image_rejects_invalid_rgba8_dimensions_and_lengths() {
        #[derive(Debug)]
        struct Case {
            width: u32,
            height: u32,
            len: usize,
        }

        let cases = [
            Case {
                width: 0,
                height: 1,
                len: 0,
            },
            Case {
                width: 1,
                height: 0,
                len: 0,
            },
            Case {
                width: u32::MAX,
                height: u32::MAX,
                len: 0,
            },
            Case {
                width: 2,
                height: 2,
                len: 15,
            },
        ];

        for case in cases {
            assert!(
                std::panic::catch_unwind(|| Image::from_rgba8(
                    case.width,
                    case.height,
                    vec![0; case.len],
                ))
                .is_err(),
                "invalid RGBA8 input must panic: {case:?}",
            );
        }
    }
}
