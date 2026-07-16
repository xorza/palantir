//! Shaped text records consumed by the native text backend.

use crate::primitives::color::ColorU8;
use crate::primitives::urect::URect;
use crate::text::TextCacheKey;
use glam::Vec2;

/// One shaped text run placed in physical-px space. The buffer it references
/// is resolved by the backend at submit time using [`TextCacheKey`] against
/// the active `TextMeasure`.
///
/// **Layout**: fields ordered so the struct is `Pod` with no internal
/// padding. `TextCacheKey` (24 B, align 8) leads so its alignment
/// requirement is satisfied without filler. Color stores **straight-alpha
/// linear** bytes: the native text backend consumes linear and premultiplies
/// at output (no sRGB roundtrip — matches the crate's colour contract), which
/// keeps the per-frame hot path Pod-shaped and lets the backend hash whole
/// `TextRun` slices via `bytemuck`.
#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct TextRun {
    pub(crate) key: TextCacheKey,
    /// Top-left of the run's bounding box, physical px.
    pub(crate) origin: Vec2,
    /// Bounds for clipping (physical px) — the parent rect after transform &
    /// snap. The backend only y-culls whole lines against this (keeps
    /// off-screen lines out of the glyph atlas); the actual pixel clip is
    /// the batch GPU scissor
    /// ([`TextBatch::scissor`](crate::renderer::render_buffer::batch::TextBatch::scissor), the union of the
    /// batch's bounds), which the composer's strict-bounds batching rule
    /// keeps no wider than any ancestor-clipped run's bounds.
    pub(crate) bounds: URect,
    pub(crate) color: ColorU8,
    /// Per-run scale factor on top of the global DPI scale, sourced from
    /// the cumulative ancestor `TranslateScale.scale` at compose time
    /// and snapped to a log-multiplicative ladder
    /// (`composer::snap_text_scale`). `1.0` outside any transformed
    /// subtree. Multiplied into the text backend's per-`TextArea.scale`, which
    /// cosmic-text mixes into its glyph `CacheKey` (`font_size * scale`),
    /// so every distinct value here mints a fresh swash rasterization +
    /// atlas slot. Snapping is what keeps a continuous zoom gesture from
    /// re-rasterizing every glyph every frame.
    pub(crate) scale: f32,
}

impl std::hash::Hash for TextRun {
    #[inline]
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        state.write(bytemuck::bytes_of(self));
    }
}
