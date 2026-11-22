//! User-supplied raster images: the pure data types. The stateful lifecycle
//! lives in [`crate::image_registry`].

pub(crate) mod error;
pub(crate) mod image_filter;
pub(crate) mod image_placement;
pub(crate) mod png_rgba;

use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::color::srgba_u8::SrgbaU8;
use crate::primitives::paint::image::error::ImageDataError;
use glam::UVec2;
#[cfg(feature = "png")]
use std::io;
use std::num::NonZeroU32;

/// A CPU pixel buffer: straight (non-premultiplied) sRGB RGBA8. The backend
/// premultiplies on upload to a `Rgba8UnormSrgb` texture, since filtering
/// straight color across a soft edge darkens it. Window icons use the same
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
    /// Panics on the size errors [`Self::from_srgba8`] returns.
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

    /// The texel at `at`, column then row.
    ///
    /// # Panics
    ///
    /// Panics unless `at` lies inside [`Self::size`].
    #[track_caller]
    pub fn texel(&self, at: UVec2) -> SrgbaU8 {
        assert!(
            at.x < self.size.x && at.y < self.size.y,
            "texel {at} lies outside an image of {}",
            self.size
        );
        self.texels()[(at.y * self.size.x + at.x) as usize]
    }

    /// This image `factor` times as large on each axis, each texel a `factor` × `factor` block,
    /// as a nearest filter shows it.
    ///
    /// # Panics
    ///
    /// Panics when the larger size makes [`Self::blank`] panic.
    #[track_caller]
    #[must_use]
    pub fn magnified(&self, factor: NonZeroU32) -> Self {
        let factor = factor.get();
        let mut out = Self::blank(self.size * factor);
        out.fill_with(|at| self.texel(at / factor));
        out
    }

    /// The texels' bytes, row-major, four a texel in `r, g, b, a` order: the layout
    /// [`Self::from_srgba8`] takes, for a codec or a clipboard that wants bytes.
    pub fn as_srgba8(&self) -> &[u8] {
        &self.pixels
    }

    /// The texels, for writing.
    pub fn texels_mut(&mut self) -> &mut [SrgbaU8] {
        bytemuck::cast_slice_mut(&mut self.pixels)
    }

    /// The PNG in `bytes`, its texels converted to 8-bit sRGB RGBA.
    ///
    /// # Errors
    ///
    /// The decoder's error, or the image's when it has no texel, as an [`io::Error`].
    #[cfg(feature = "png")]
    pub fn from_png(bytes: &[u8]) -> io::Result<Self> {
        let mut pixels = Vec::new();
        let size = png_rgba::decode(bytes, &mut pixels).map_err(|error| match error {
            png::DecodingError::IoError(error) => error,
            error => io::Error::other(error),
        })?;
        Self::from_srgba8(size, pixels).map_err(io::Error::other)
    }

    /// Write this image to `out` as an 8-bit RGBA PNG, its bytes as they are.
    ///
    /// # Errors
    ///
    /// The encoder's or the writer's error, as an [`io::Error`].
    #[cfg(feature = "png")]
    pub fn write_png(&self, mut out: impl io::Write) -> io::Result<()> {
        png_rgba::encode(&self.pixels, self.size, &mut out).map_err(|error| match error {
            png::EncodingError::IoError(error) => error,
            error => io::Error::other(error),
        })?;
        out.flush()
    }

    /// Every texel from its position, column then row, row by row.
    pub fn fill_with(&mut self, mut texel: impl FnMut(UVec2) -> SrgbaU8) {
        let width = self.size.x as usize;
        for (row, line) in self.texels_mut().chunks_exact_mut(width).enumerate() {
            for (col, out) in line.iter_mut().enumerate() {
                *out = texel(UVec2::new(col as u32, row as u32));
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

    /// The image scaled down so that its longer side is `max_side` texels, each side rounded to
    /// the nearest texel and at least one; a copy when it fits already. A tent filter widened by
    /// the ratio weighs every source texel and adds no ringing at the hard edges a UI is made of;
    /// it averages premultiplied linear light, as the frame blends, since an average of encoded
    /// bytes darkens every edge between light and dark. Rows go through one at a time, decoded and
    /// filtered across as the tent first reads them, so the scratch is a few rows, not the image.
    #[must_use]
    pub fn fitted(&self, max_side: NonZeroU32) -> Self {
        let size = fitted_size(self.size, max_side.get());
        if size == self.size {
            return self.clone();
        }
        let (from_w, from_h) = (self.size.x as usize, self.size.y as usize);
        let (to_w, to_h) = (size.x as usize, size.y as usize);
        // Every output column's taps, flat: column `x` reads `x_taps[x_starts[x]..x_starts[x + 1]]`.
        let mut taps = Vec::new();
        let mut x_taps = Vec::new();
        let mut x_starts = Vec::with_capacity(to_w + 1);
        x_starts.push(0);
        for x in 0..to_w {
            tent_taps(x, from_w, to_w, &mut taps);
            x_taps.extend_from_slice(&taps);
            x_starts.push(x_taps.len());
        }
        // The source rows filtered across, in a ring as deep as the most rows one output row's
        // tent reads: `ceil(center + ratio) - floor(center - ratio)` is at most `2 · ratio + 2`.
        let ring_rows = (2 * from_h).div_ceil(to_h) + 2;
        let mut ring = vec![[0.0; 4]; ring_rows * to_w];
        let mut line = vec![[0.0; 4]; from_w];
        let mut filtered = 0;
        let mut fitted = Self::blank(size);
        for y in 0..to_h {
            tent_taps(y, from_h, to_h, &mut taps);
            let first = taps.first().expect("a tent reads a source row").0;
            let last = taps.last().expect("a tent reads a source row").0 + 1;
            debug_assert!(
                last - first <= ring_rows,
                "the ring holds every row a tent reads"
            );
            while filtered < last {
                let source = &self.texels()[filtered * from_w..][..from_w];
                for (linear, texel) in line.iter_mut().zip(source) {
                    let RgbaF32 { r, g, b, a } = RgbaF32::from_srgba8(*texel);
                    *linear = [r * a, g * a, b * a, a];
                }
                let slot = &mut ring[(filtered % ring_rows) * to_w..][..to_w];
                for (x, out) in slot.iter_mut().enumerate() {
                    *out = weighed(&x_taps[x_starts[x]..x_starts[x + 1]], |i| line[i]);
                }
                filtered += 1;
            }
            for (x, out) in fitted.row_mut(y as u32).iter_mut().enumerate() {
                let [r, g, b, a] = weighed(&taps, |i| ring[(i % ring_rows) * to_w + x]);
                let straight = if a > 0.0 { a.recip() } else { 0.0 };
                *out = RgbaF32::new(r * straight, g * straight, b * straight, a).to_srgba8();
            }
        }
        fitted
    }
}

/// `size` scaled so that its longer side is `max_side`, each side rounded to the nearest and at
/// least 1, or `size` when it fits.
fn fitted_size(size: UVec2, max_side: u32) -> UVec2 {
    let longer = size.x.max(size.y);
    if longer <= max_side {
        return size;
    }
    let side = |len: u32| {
        let scaled =
            (u64::from(len) * u64::from(max_side) + u64::from(longer) / 2) / u64::from(longer);
        u32::try_from(scaled.max(1)).expect("a scaled side is at most `max_side`")
    };
    UVec2::new(side(size.x), side(size.y))
}

/// The source texels output texel `at` of `to` reads from `from` along one axis, with their
/// normalized weights: a tent on the output texel's center, as wide as the ratio.
#[expect(
    clippy::cast_sign_loss,
    reason = "each bound is floored at zero first, and `as` saturates the far side"
)]
fn tent_taps(at: usize, from: usize, to: usize, taps: &mut Vec<(usize, f32)>) {
    taps.clear();
    let ratio = from as f32 / to as f32;
    let center = (at as f32 + 0.5) * ratio;
    let first = (center - ratio).floor().max(0.0) as usize;
    let last = ((center + ratio).ceil().max(0.0) as usize).min(from);
    taps.extend((first..last).filter_map(|i| {
        let weight = 1.0 - ((i as f32 + 0.5 - center) / ratio).abs();
        (weight > 0.0).then_some((i, weight))
    }));
    let total: f32 = taps.iter().map(|(_, weight)| weight).sum();
    for (_, weight) in taps.iter_mut() {
        *weight /= total;
    }
}

/// The sum of `texel(i)` over `taps`, each by its weight.
fn weighed(taps: &[(usize, f32)], texel: impl Fn(usize) -> [f32; 4]) -> [f32; 4] {
    taps.iter().fold([0.0; 4], |mut sum, &(i, weight)| {
        for (out, channel) in sum.iter_mut().zip(texel(i)) {
            *out += channel * weight;
        }
        sum
    })
}

fn rgba8_len(size: UVec2) -> Result<usize, ImageDataError> {
    if size.x == 0 || size.y == 0 {
        return Err(ImageDataError::ZeroSize { size });
    }
    u64::from(size.x)
        .checked_mul(u64::from(size.y))
        .and_then(|texels| texels.checked_mul(4))
        .and_then(|len| usize::try_from(len).ok())
        .ok_or(ImageDataError::LengthOverflow { size })
}

#[cfg(feature = "internals")]
pub(crate) mod internals {
    use glam::UVec2;

    use crate::primitives::paint::color::srgba_u8::SrgbaU8;
    use crate::primitives::paint::image::Image;

    impl Image {
        /// The `size` texels at `min` and past it, as a test compares a region with a whole read.
        ///
        /// # Panics
        ///
        /// Unless the region lies inside the image and holds a texel.
        #[track_caller]
        #[must_use]
        pub fn cropped(&self, min: UVec2, size: UVec2) -> Self {
            let mut crop = Self::blank(size);
            crop.fill_with(|at| self.texel(min + at));
            crop
        }

        /// Each texel with where it lies, row by row, as a test walks every pixel of a read.
        pub fn enumerate_texels(&self) -> impl Iterator<Item = (UVec2, SrgbaU8)> + '_ {
            let width = self.size.x;
            self.texels().iter().enumerate().map(move |(idx, &texel)| {
                let idx = idx as u32;
                (UVec2::new(idx % width, idx / width), texel)
            })
        }
    }
}

#[cfg(test)]
mod tests;
