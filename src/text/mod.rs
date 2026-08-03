//! Text shaping & measurement.
//!
//! Two shaping backends, one struct each:
//!
//! - [`cosmic::CosmicMeasure`] — real shaping via `cosmic-text`, with a
//!   per-key shaped-buffer cache. The wgpu backend replays these buffers
//!   through [`render`]. Each render run carries its record-local
//!   source span so an encoded-cache miss can restore an evicted shaped
//!   buffer. The only production backend.
//! - [`mono::internals::measure`] — deterministic placeholder metric behind
//!   the test/internals-only `TextShaper::test_mono`. Every glyph is
//!   `font_size_px * 0.5` wide; it mints no shaped buffer, so `TextSystem`
//!   reports [`key::TextShapeKey::INVALID`] for those runs and the renderer
//!   drops them. Lets the engine run in tests and headless tools without a
//!   font system.
//!
//! There's no `TextMeasure` trait: the render path needs `CosmicMeasure`'s
//! shaped buffers + font system (leased cosmic-free through
//! [`render`]), which the mono fallback cannot provide,
//! so a trait would just be a downcast in disguise.
//!
//! Module layout: this file owns only the vocabulary every other module
//! names ([`FontFamily`], [`FontWeight`]) plus the two constants the
//! renderer has to agree with. One major type per module beyond that —
//! [`shaper`] the app-global coordinator, [`system`] the per-window reuse
//! slots, [`request`] what a shaping call is asked, [`root`] what an
//! unbounded shape answers, [`key`] the quantized cache identity,
//! [`shaped_ref`] the render handoff, [`probe`] the public geometry
//! surface over [`layout_probe`]'s lease, [`render`] the cosmic-free
//! render vocabulary, [`wrap`] the wrap-policy vocabulary.

#[cfg(feature = "internals")]
pub(crate) mod bench;
mod cache_probe;
mod cosmic;
pub(crate) mod key;
pub(crate) mod layout_probe;
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

/// Additive step on the text-scale ladder used by the composer to snap
/// continuous zoom scales to discrete glyph-cache keys (`composer::
/// snap_text_scale`). The cascade computes text damage rects at the
/// unscaled cascade scale; the composer paints glyphs at the snapped
/// scale — between rungs the painted block can be up to
/// `TEXT_SCALE_STEP / 2` wider than the damage rect on each axis.
/// [`crate::scene::shapes::record::text_paint_bbox_local`] inflates
/// by this fraction to keep damage covering the worst-case painted
/// extent.
///
/// Single source — `composer::TEXT_SCALE_STEP` re-exports this value.
pub(crate) const TEXT_SCALE_STEP: f32 = 0.005;

/// Frames a *rendered* run's cached artifacts survive untouched: the
/// shaped buffer ([`cosmic::PROTECTED_KEEP_FRAMES`]) and the glyph
/// template the backend encodes from it
/// (`renderer::backend::text::encode::ENCODED_CACHE_KEEP_FRAMES`).
///
/// One value rather than two, because the two windows are not
/// independent. The encoded cache is what generates the render-side
/// buffer lookups, so a buffer whose window expired first would be
/// silently restored from source on a hit the encoder had already
/// counted as free — the retention would still be *correct*, just
/// quietly paying to reshape. Deriving both from here is what keeps
/// that from drifting apart in a later edit to one of them.
///
/// **A shared constant is only half of that, and for a while it was the
/// only half.** Each cache also counted its own frames, off different
/// events: this side ticked on the record path (`FullRecord` frames
/// only), the encoder's on the submit path — which additionally runs
/// for `PaintOnly` frames and used to skip any frame that prepared no
/// text batch. Equal numbers over unequal clocks, so a recorded frame
/// that drew no text aged buffers and not encoded entries, and the
/// ordering this constant exists to guarantee simply did not hold.
/// Both now read one clock — see
/// [`ShaperInner::frame`](shaper::ShaperInner) — which is what makes
/// the equality mean something.
///
/// Lives here rather than with either cache because `renderer` depends
/// on `text` and not the reverse, so this is the only spot both can
/// name. Same reason as [`TEXT_SCALE_STEP`].
pub(crate) const RENDERED_RUN_KEEP_FRAMES: u64 = 120;

/// Font family picker on [`crate::TextStyle`] and
/// [`crate::Shape::Text`]. `Sans` resolves to bundled Inter (the default
/// proportional face); `Mono` resolves to bundled JetBrains Mono. Both
/// ship inside `CosmicMeasure::with_bundled_fonts`; the test-only mono
/// fallback ignores family entirely.
/// Weight (Regular/Bold) is an independent axis — see [`FontWeight`].
///
/// `#[repr(u8)]` with explicit discriminants pins the on-disk tag so
/// `TextShapeKey::family_q` and the `ShapeRecord::Text` hash byte
/// stay stable across variant reordering.
#[repr(u8)]
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum FontFamily {
    #[default]
    Sans = 0,
    Mono = 1,
}

/// Font weight picker on [`crate::TextStyle`] and [`crate::Shape::Text`],
/// independent of [`FontFamily`]. `Regular` shapes with the family's
/// normal face; `Bold` requests the bold face (a distinct static face
/// for Inter, an instantiated `wght` for the variable JetBrains
/// Mono) via cosmic-text's `Attrs::weight`.
///
/// `#[repr(u8)]` pins the tag for `TextShapeKey::weight_q` and the
/// `ShapeRecord::Text` hash byte.
#[repr(u8)]
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum FontWeight {
    #[default]
    Regular = 0,
    Bold = 1,
}

#[cfg(test)]
mod tests;
