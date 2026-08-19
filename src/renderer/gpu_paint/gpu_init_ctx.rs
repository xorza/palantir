//! What a `GpuPaint` gets once, to build its pipelines.

use crate::text::shaper::TextShaper;

/// Handed to [`GpuPaint::init`](crate::renderer::gpu_paint::GpuPaint::init). Carries what's needed to build
/// format-dependent pipelines, and the window's own text shaper.
#[derive(Debug)]
pub struct GpuInitCtx<'a> {
    pub device: &'a wgpu::Device,
    /// The off-screen color target's format (sRGB `Rgba8UnormSrgb`). Match
    /// it on your render pipeline's color target.
    pub target_format: wgpu::TextureFormat,
    /// The shaper the rest of the window draws its text with.
    ///
    /// A view drawing text of its own — a label pinned to a point in a scene, a
    /// dimension on a drawing — asks this for glyph placements and bitmaps
    /// through [`TextShaper::glyphs`], and packs them into an atlas and a
    /// pipeline of its own. Palantir renders text for the widgets it owns and
    /// cannot reach inside a `GpuView`, so the alternative to sharing this is a
    /// second font stack in the same process: another scan of the platform's
    /// fonts, and a view whose labels silently disagree with the UI around them.
    ///
    /// Handed over at init because it is a handle worth keeping — clone it and
    /// hold it. Taking a lease is per-batch work and belongs in
    /// [`GpuPaint::paint`](crate::renderer::gpu_paint::GpuPaint::paint), where the raster scale is also known.
    pub text: &'a TextShaper,
}
