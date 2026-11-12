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

#[cfg(feature = "internals")]
pub(crate) mod bench;
// Private so no cosmic type is nameable outside `crate::text`. `pub(crate)` inside the directory therefore reaches `crate::text` only, which is what a sibling of `cosmic` needs.
mod cosmic;
pub(crate) mod error;
pub(crate) mod extent;
pub(crate) mod family_keys;
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
#[cfg(feature = "internals")]
mod mono;
pub(crate) mod probe;
pub(crate) mod render;
pub(crate) mod request;
pub(crate) mod root;
pub(crate) mod run;
pub(crate) mod shaped_ref;
pub(crate) mod shaped_text;
pub(crate) mod shaper;
pub(crate) mod shaping_font;
pub(crate) mod system;
pub(crate) mod wrap;

use crate::primitives::geometry::size::Size;

/// Additive step on the text-scale ladder. The composer snaps a continuous zoom scale to a rung before it picks a glyph-cache key (`composer::geometry::snap_text_scale`).
///
/// Additive, not proportional: fine rungs where every percent shows (0.5% at 1×), coarse rungs and fewer atlas keys at low zoom.
///
/// Measurement uses the unscaled `font_size`; only paint snaps, so the glyph block is up to `TEXT_SCALE_STEP / 2` off the layout rect per axis. `TextDrawRow.bounds` clips the excess and `inflate_text_damage` inflates damage by the same fraction.
pub(crate) const TEXT_SCALE_STEP: f32 = 0.005;

/// How far a run inked over `inked` (local logical px) can paint past it per side, in the logical
/// px of its screen. The snap moves the scale by at most `TEXT_SCALE_STEP / 2`, absolute, so the
/// painted block moves by `inked × STEP/2` whatever the zoom: padding the transformed size instead
/// would shrink the slack below scale 1 and cut glyphs.
pub(crate) fn snap_slack(inked: Size) -> Size {
    Size::new(
        inked.w * (TEXT_SCALE_STEP * 0.5),
        inked.h * (TEXT_SCALE_STEP * 0.5),
    )
}

/// Frames a *rendered* run's shaped buffer survives untouched: the floor of the cache's protected tier (each entry adds its share of [`RENDERED_RUN_KEEP_SPREAD_MASK`]), and the ceiling the glyph-template window (`gpu::raster::text_backend::encode::cache::ENCODED_CACHE_KEEP_FRAMES`) must stay under.
///
/// Ordered, not equal: if the buffer's expired first, a miss the encoder counted as cheap would reshape from source. Both count the host's frames off the cache's one clock, not wall time. Lives here because `renderer` depends on `text`, not the reverse.
pub(crate) const RENDERED_RUN_KEEP_FRAMES: u64 = 120;

/// Extra frames a shaped buffer keeps past [`RENDERED_RUN_KEEP_FRAMES`], masked out of the run's own key.
///
/// A shared deadline makes reclamation bursty: a page switch promotes hundreds of runs on one frame, and they all fall due together. Sixteen deadlines spread that cost.
///
/// Masked from the key, not counted, so the offset is stable per entry: a rotating counter could move a deadline inward on re-promotion. Costs a larger expiry ring.
pub(crate) const RENDERED_RUN_KEEP_SPREAD_MASK: u64 = 15;

/// Frames a *probationary* entry survives before `ShapedBufferCache::tick_frame` drops it:
/// inserted and never looked up, or superseded (`ShapedBufferCache::supersede`) after its reuse
/// slot moved to another key. Short on purpose: a drag mints a never-reused key per run per
/// frame, and the protected window would hold `runs × RENDERED_RUN_KEEP_FRAMES` dead buffers.
///
/// Supersession is what makes this window reach that population: layout shapes a run and the
/// encoder renders it on the same frame, which is a lookup, so every buffer would be promoted at
/// creation. `TextSystem` alone can tell "wants a different shape" (dead) from "left the tree"
/// (may return). It is a demotion, not an eviction: four frames of grace let an oscillating
/// label or a reversing drag still hit.
///
/// Refused: reference-counted retention. `EncodedKey` embeds [`key::TextShapeKey`], so a width
/// drag mints an encoded entry per frame; holding its buffer strongly pins
/// `runs × (window + 1)` buffers, holding it weakly leaves buffers dying under live entries, so
/// the restore path (`ShapedTextRef`, `InternedText`, `CosmicMeasure::ensure_buffer`) must stay.
pub(crate) const PROBATION_KEEP_FRAMES: u64 = 4;

#[cfg(feature = "internals")]
pub(crate) mod internals {
    use crate::common::expiry_wheel::ExpiryWheel;
    use crate::text::cosmic::shaped_buffer_cache;
    use crate::text::key::TextShapeKey;

    /// Frames per revolution of the shaped-buffer expiry ring; audits warm and measure in whole revolutions.
    pub const SHAPED_BUFFER_RING_FRAMES: usize =
        ExpiryWheel::<TextShapeKey>::slots_for_keep(shaped_buffer_cache::KEEP_FRAMES) as usize;
}

#[cfg(test)]
mod tests;
