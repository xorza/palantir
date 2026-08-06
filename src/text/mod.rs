//! Text shaping & measurement.
//!
//! Two shaping backends, one struct each:
//!
//! - [`cosmic::CosmicMeasure`] — real shaping via `cosmic-text`, with a
//!   per-key shaped-buffer cache. The wgpu backend replays these buffers
//!   through [`render`]. Each render run carries its record-local
//!   source span so an encoded-cache miss can restore an evicted shaped
//!   buffer. The only production backend.
//! - [`mono::measure`] — deterministic placeholder metric behind
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
//! # Module layout
//!
//! Two shapes, and a module is one or the other.
//!
//! **Owner modules** are named for the one type they own, and hold that
//! type's private helpers and nothing else: [`shaper`] the app-global
//! coordinator, [`system`] the per-window reuse slots, [`request`] what a
//! shaping call is asked, [`root`] what an unbounded shape answers,
//! [`key`] the quantized cache identity, [`shaped_ref`] the render
//! handoff, [`run`] how a caller describes a run to probe, [`probe`] the
//! public geometry surface over its own `layout` lease.
//!
//! **Vocabulary modules** hold the small value types one layer speaks in,
//! where naming a module per type would scatter a set that is only ever
//! read together: this file ([`FontFamily`], [`FontWeight`], plus the two
//! constants the renderer has to agree with), [`wrap`] the wrap policies,
//! [`render`] the cosmic-free render terms.
//!
//! A backend gets a directory, because one type with five separable jobs
//! is neither shape. [`cosmic`] is `CosmicMeasure` plus wrapped shaping
//! in `mod.rs`, with `retention` (the two-tier windows and the four
//! operations that move an entry between them), `truncate` (the
//! cluster-precise cut), `geometry` (reading measurements back off a
//! shaped buffer), `glyphs` (the render side), and `counters` beside it.
//! Its children reach the measurer's private fields directly — privacy
//! descends — so the split costs no widening, and each file is one
//! answerable question.

#[cfg(feature = "bench")]
pub(crate) mod bench;
mod cosmic;
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

/// Frames a *rendered* run's shaped buffer survives untouched — the
/// protected tier of the shaped-buffer cache
/// ([`cosmic::PROTECTED_KEEP_FRAMES`]), and the ceiling the backend's
/// glyph-template window
/// (`renderer::backend::text::encode::ENCODED_CACHE_KEEP_FRAMES`) must
/// stay under.
///
/// The two windows are not independent, but the relation between them
/// is an **ordering, not an equality**. The encoded cache is what
/// generates the render-side buffer lookups, so a buffer whose window
/// expired first would be silently restored from source on a miss the
/// encoder had already counted as cheap — still *correct*, just quietly
/// paying to reshape. Everything that matters is preserved by keeping
/// the buffer window the longer one; the encoded side is free to sit
/// below it, and does, because its window doubles as its population
/// multiplier. The `const _` assertion beside that constant is the
/// tripwire.
///
/// **Comparable numbers are only half of an ordering.** Both windows
/// count frames off **one** clock — [`ShaperInner::frame`](shaper::ShaperInner)
/// — which is what makes comparing them mean anything. Counting off
/// per-cache events instead (this side on the record path, the encoder's
/// on the submit path, which also runs for `PaintOnly` frames) puts the
/// two windows on unequal clocks, and the ordering stops holding: a
/// recorded frame that drew no text would age buffers but not encoded
/// entries.
///
/// The ordering is deliberately **not** an equality. Sharing one value
/// would conflate "cannot cross" with "must match" and cost the encoded
/// cache four times the resident rows it needs — the shaped side has a
/// probation tier to shed gesture churn, and the encoded side has none.
///
/// Lives here rather than with either cache because `renderer` depends
/// on `text` and not the reverse, so this is the only spot both can
/// name. Same reason as [`TEXT_SCALE_STEP`].
pub(crate) const RENDERED_RUN_KEEP_FRAMES: u64 = 120;

/// Font family picker on [`crate::TextStyle`] and
/// [`Shape::text`](crate::Shape::text). `Sans` resolves to bundled Inter (the default
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

/// Font weight picker on [`crate::TextStyle`] and
/// [`Shape::text`](crate::Shape::text),
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
