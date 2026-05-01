//! Text shaping & measurement.
//!
//! Phase 1 scope: produce a [`Size`] for a `(text, font_size, max_width)`
//! triple so leaf widgets can report a real intrinsic size to layout.
//!
//! [`TextMeasure`] is a trait so the engine can stay font-agnostic in tests:
//! [`MonoMeasure`] keeps the historical 8 px/char × 16 px-line placeholder so
//! existing layout tests continue to pin exact pixel values without bundling a
//! font. Examples and apps that want real text install [`CosmicMeasure`],
//! which shapes via `cosmic-text` and caches the resulting `Buffer` keyed on
//! the inputs that affect shaping. Steady-state shaping is alloc-free once
//! every visible string has been seen at least once.
//!
//! Rendering (atlas / glyph quads) is a separate concern handled by the wgpu
//! backend in a follow-up step; this module only does CPU shaping +
//! measurement.

use crate::primitives::Size;

mod cosmic;
mod mono;

pub use cosmic::CosmicMeasure;
pub use mono::MonoMeasure;

/// Pluggable text measurement strategy. Implementors return the bounding size
/// of `text` rendered at `font_size_px`, optionally constrained by
/// `max_width_px` (which triggers wrapping when supplied).
///
/// The boxed-trait approach lets tests use [`MonoMeasure`] for deterministic
/// 8 px/char metrics without pulling in a font, while real apps install
/// [`CosmicMeasure`] for true shaping. The trait is intentionally tiny — it's
/// the seam between authoring (which writes `Shape::Text.measured`) and
/// whichever shaping engine is in use.
pub trait TextMeasure {
    fn measure(&mut self, text: &str, font_size_px: f32, max_width_px: Option<f32>) -> Size;
}

impl<T: TextMeasure + ?Sized> TextMeasure for Box<T> {
    fn measure(&mut self, text: &str, font_size_px: f32, max_width_px: Option<f32>) -> Size {
        (**self).measure(text, font_size_px, max_width_px)
    }
}
