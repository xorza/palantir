//! User-supplied raster images — the pure data types.
//!
//! [`Image`] is a decoded pixel buffer and [`ImageFit`] is the
//! intrinsic-size-to-rect mapping. The stateful lifecycle (registration,
//! GPU upload/release, the RAII `ImageHandle`, the `TextureId` identity)
//! lives in [`crate::renderer::image_registry`] — `primitives` stays a
//! pure leaf.

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
/// one bind group per texture.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ImageFilter {
    #[default]
    Linear,
    Nearest,
}

/// Decoded pixel buffer. Straight (non-premultiplied) sRGB RGBA8 — the
/// backend uses a `Rgba8UnormSrgb` texture so the sampler decodes to
/// linear on read, and the shader premultiplies. Dropped right after the
/// backend uploads it to GPU.
#[derive(Debug)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

impl Image {
    /// Build from raw RGBA8 bytes. Hard-asserts non-zero dimensions and
    /// `pixels.len() == width * height * 4`.
    pub fn from_rgba8(width: u32, height: u32, pixels: Vec<u8>) -> Self {
        // A 0×0 image passes the length check (0 == 0) but is a logic
        // error: it would hit a wgpu validation panic at upload, a frame
        // later and far from the cause. Fail here, at construction.
        assert!(
            width > 0 && height > 0,
            "Image::from_rgba8: image dimensions must be non-zero, got {width}x{height}",
        );
        let expected = (width as usize) * (height as usize) * 4;
        assert_eq!(
            pixels.len(),
            expected,
            "Image::from_rgba8: pixels.len() = {} != width*height*4 = {}",
            pixels.len(),
            expected,
        );
        Self {
            width,
            height,
            pixels,
        }
    }
}
