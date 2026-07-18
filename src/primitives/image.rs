//! User-supplied raster images — the pure data types.
//!
//! [`Image`] is a decoded pixel buffer and [`ImageFit`] is the
//! intrinsic-size-to-rect mapping. The stateful lifecycle (registration,
//! GPU upload/release, the RAII `ImageHandle`, the `TextureId` identity)
//! lives in [`crate::renderer::image_registry`] — `primitives` stays a
//! pure leaf.

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
    /// preserved. Default — matches the legacy "no fit" behaviour.
    #[default]
    Fill,
    /// Preserve aspect ratio; fit the image entirely inside the rect.
    /// Letterboxes (transparent margins) if aspect ratios differ.
    Contain,
    /// Preserve aspect ratio; fill the rect entirely. Crops the
    /// image's longer axis (centered).
    Cover,
    /// Paint at the image's intrinsic pixel size, centered in the rect.
    /// Larger-than-rect images overflow the rect (currently uncropped —
    /// future slice can add per-image scissor).
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
/// checkerboards, pixel peeping. Implemented as a UV texel-center
/// snap in the image shader, so both filters share one sampler and
/// one bind group per texture. Serde (lowercase) so hosts can persist
/// a filter choice in their config files.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ImageFilter {
    #[default]
    Linear,
    Nearest,
}

/// Decoded pixel buffer. Straight (non-premultiplied) sRGB RGBA8 — the backend
/// uses a `Rgba8UnormSrgb` texture so the sampler decodes to linear on read,
/// and the shader premultiplies. Window icons use the same validated storage.
/// Dropped right after the backend uploads a registered image to GPU.
#[derive(Clone, Debug)]
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
        assert!(
            width != 0 && height != 0,
            "RGBA8 dimensions must be non-zero, got {width}x{height}",
        );
        let expected = u64::from(width)
            .checked_mul(u64::from(height))
            .and_then(|texels| texels.checked_mul(4))
            .and_then(|len| usize::try_from(len).ok())
            .expect("RGBA8 dimensions overflow addressable byte length");
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
}

#[cfg(test)]
mod tests {
    use crate::primitives::image::Image;

    #[test]
    fn image_stores_valid_rgba8_dimensions_and_pixels() {
        let pixels = vec![255, 0, 0, 255, 0, 255, 0, 128];
        let image = Image::from_rgba8(2, 1, pixels.clone());
        assert_eq!(image.size, glam::UVec2::new(2, 1));
        assert_eq!(image.pixels, pixels);
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
