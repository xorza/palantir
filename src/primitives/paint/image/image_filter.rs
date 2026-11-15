//! How an image samples its texels: [`ImageFilter`] where it scales, [`ImageDownsample`] where it
//! minifies.

/// How texels are interpolated when an image paints at a size other than its
/// intrinsic one; [`ImagePaint`](crate::ImagePaint) picks it
/// separately for minification and magnification. Implemented as a UV
/// texel-center snap, so every combination shares one sampler and bind group.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
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
/// One bilinear tap reads a 2×2 neighborhood however far the image shrinks,
/// and which texels it reads moves with the fractional UV, so panning makes
/// fine detail scintillate. Spreading taps across the pixel's derivative
/// footprint stops that.
///
/// Opt in via [`ImagePaint::downsample`](crate::ImagePaint::downsample);
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
    /// 25× of its peak). Whole taps win, keeping color, but it reads brighter
    /// than the true average.
    Peak,
}
