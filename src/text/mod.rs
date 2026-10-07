//! Text shaping & measurement.
//!
//! Two backends:
//!
//! - [`cosmic::CosmicMeasure`]: real shaping via `cosmic-text` with a per-key shaped-buffer cache, replayed by the wgpu backend through [`render`]. Each render run carries its record-local source span so an encoded-cache miss can restore an evicted buffer. The only production backend.
//! - `mono`: deterministic placeholder metric behind the test/internals-only `TextShaper::test_mono`. Every glyph is `font_size * 0.5` wide and no shaped buffer is minted, so the renderer drops those runs. It replaces measurement, not the font system.
//!
//! There is no `TextMeasure` trait: the render path needs `CosmicMeasure`'s shaped buffers and font system, which mono cannot provide.
//!
//! # Module layout
//!
//! Owner modules hold one type each: [`shaper`] (app-global coordinator), [`system`] (per-window reuse slots), [`key`] (quantized cache identity), [`glyphs`] (the render-side lease), [`font_scope`] and [`font_scan`] (which faces a database starts with, and one built on another thread), among others.
//!
//! Vocabulary modules are small value types: this file's constants, [`wrap`], [`render`], the face axes and [`error`].
//!
//! [`cosmic`] is a directory around `CosmicMeasure`; its children reach the private fields directly.

#[cfg(feature = "bench")]
pub(crate) mod bench;
// Private so no cosmic type is nameable outside `crate::text`. `pub(crate)` inside the directory therefore reaches `crate::text` only, which is what a sibling of `cosmic` needs.
mod cosmic;
pub(crate) mod error;
pub(crate) mod extent;
pub(crate) mod font_family;
// Gated with its only consumer, the winit host.
#[cfg(feature = "winit")]
pub(crate) mod font_scan;
pub(crate) mod font_scope;
pub(crate) mod font_slant;
pub(crate) mod font_source;
pub(crate) mod font_weight;
pub(crate) mod glyph_font;
pub(crate) mod glyphs;
pub(crate) mod key;
#[cfg(any(test, feature = "internals"))]
mod mono;
pub(crate) mod probe;
pub(crate) mod render;
pub(crate) mod request;
pub(crate) mod root;
pub(crate) mod run;
pub(crate) mod shaped_ref;
pub(crate) mod shaper;
pub(crate) mod system;
pub(crate) mod wrap;

/// Additive step on the text-scale ladder. The composer snaps a continuous zoom scale to a rung before it picks a glyph-cache key (`composer::geometry::snap_text_scale`).
///
/// Additive, not proportional: the step shrinks as a percent of size as zoom grows (0.005/4 ≈ 0.125% at 4×, 0.5% at 1×, 1% at 0.5×). Fine rungs where every percent shows, coarse rungs and fewer atlas keys at low zoom.
///
/// Measurement uses the unscaled `font_size`; only paint snaps. At a non-rung zoom the glyph block is up to `TEXT_SCALE_STEP / 2` off the layout rect per axis. `TextDrawRow.bounds` clips the excess and [`crate::shape::record::text_paint_bbox_local`] inflates damage by the same fraction.
pub(crate) const TEXT_SCALE_STEP: f32 = 0.005;

/// Frames a *rendered* run's shaped buffer survives untouched: the floor of the cache's protected tier (each entry adds its share of [`RENDERED_RUN_KEEP_SPREAD_MASK`]), and the ceiling the glyph-template window (`gpu::raster::text_backend::encode::cache::ENCODED_CACHE_KEEP_FRAMES`) must stay under.
///
/// The two windows are ordered, not equal: if the buffer's expired first, a miss the encoder counted as cheap would reshape from source. A `const _` assertion beside the encoded constant is the tripwire.
///
/// Both count frames off the shaped-buffer cache's one clock; per-cache event counts would desynchronize them. A frame is the host's, not wall time.
///
/// Lives here because `renderer` depends on `text`, not the reverse.
pub(crate) const RENDERED_RUN_KEEP_FRAMES: u64 = 120;

/// Extra frames a shaped buffer keeps past [`RENDERED_RUN_KEEP_FRAMES`], masked out of the run's own key.
///
/// A shared deadline makes reclamation bursty: a page switch promotes hundreds of runs on one frame, and they all fall due together, so one frame frees what a whole navigation created. Sixteen deadlines spread that cost over time.
///
/// Masked from the key, not counted, so the offset is stable per entry: a rotating counter could move a deadline inward on re-promotion, which the expiry wheel owes a fresh ticket for. The wheel is sized from the longest deadline, so this costs a larger ring.
pub(crate) const RENDERED_RUN_KEEP_SPREAD_MASK: u64 = 15;

#[cfg(any(test, feature = "internals"))]
pub(crate) mod internals {
    use crate::text::cosmic::shaped_buffer_cache;

    /// The shaped-buffer cache's short window; see `crate::internals::PROBATION_KEEP_FRAMES`.
    pub(crate) const PROBATION_KEEP_FRAMES: u64 = shaped_buffer_cache::PROBATION_KEEP_FRAMES;

    /// Frames one revolution of the expiry ring takes; see `crate::internals::SHAPED_BUFFER_RING_FRAMES`.
    pub(crate) const SHAPED_BUFFER_RING_FRAMES: u64 = shaped_buffer_cache::internals::RING_FRAMES;
}

#[cfg(test)]
mod tests;
