//! Text shaping & measurement.
//!
//! Two shaping backends, one struct each:
//!
//! - [`cosmic::CosmicMeasure`] — real shaping via `cosmic-text`, with a
//!   per-key shaped-buffer cache. The wgpu backend replays these buffers
//!   through [`render`]. Each render run carries its record-local
//!   source span so an encoded-cache miss can restore an evicted shaped
//!   buffer. The only production backend.
//! - [`mono`] — deterministic placeholder metric behind
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
//! public geometry surface over the lease it holds, [`glyphs`] the
//! render-side lease itself — held by the wgpu backend and by a
//! `GpuView` drawing its own text, which want the same answers off the
//! same borrow.
//!
//! **Vocabulary modules** hold the small value types one layer speaks in,
//! where naming a module per type would scatter a set that is only ever
//! read together: this file ([`FontFamily`], [`FontWeight`], plus the two
//! constants the renderer has to agree with), [`wrap`] the wrap policies,
//! [`render`] the cosmic-free render terms.
//!
//! A backend gets a directory, because one type with five separable jobs
//! is neither shape. [`cosmic`] is `CosmicMeasure` plus wrapped shaping,
//! retention and truncation in `mod.rs`, with `cache_entry` (one resident
//! shaped buffer), `cluster_glyph` (the cluster-precise cut's glyph view
//! and prefix scan), `ellipsis_memo` (the reshaped "…" advance),
//! `geometry` (reading measurements back off a shaped buffer), and
//! `counters` beside it. Its children reach the measurer's private fields
//! directly — privacy descends — so the split costs no widening, and each
//! file is one answerable question.

#[cfg(feature = "bench")]
pub(crate) mod bench;
// Private on purpose: no cosmic type is nameable outside `crate::text`, and
// this declaration is what enforces it. `pub(crate)` inside the directory
// therefore means "as far as the ladder reaches without `pub(in path)`" —
// `crate::text`, not the crate — and is what a consumer in a sibling of
// `cosmic` needs, since `pub(super)` there stops at `cosmic` itself.
mod cosmic;
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

/// Additive step on the text-scale ladder. The composer snaps a
/// continuous zoom scale to a rung of this ladder before it picks a
/// glyph-cache key (`composer::snap_text_scale`).
///
/// **Additive, not proportional.** The same step in *scale units* across
/// the range makes the step in *percent of current size* shrink as zoom
/// grows — 0.005/4 ≈ 0.125% at 4×, 0.5% at 1×, 1% at 0.5×. That is the
/// trade the perceptual case asks for: at high zoom every percent of
/// size change is visible, so the rungs want to be fine, and at low zoom
/// text is small enough that crispness stepping does not read, so coarse
/// rungs and fewer atlas keys win.
///
/// **Geometric note.** Measurement uses the unscaled `font_size_px` —
/// only the paint-time scale snaps. At a non-rung zoom the painted glyph
/// block is up to `TEXT_SCALE_STEP / 2` wider or narrower on each axis
/// than the layout-space rect it nominally fills. `TextDrawRow.bounds`
/// clips the extra width, and
/// [`crate::scene::shapes::record::text_paint_bbox_local`] inflates text
/// damage rects by the same fraction, so a rung jump between consecutive
/// frames repaints every affected pixel.
pub(crate) const TEXT_SCALE_STEP: f32 = 0.005;

/// Frames a *rendered* run's shaped buffer survives untouched — the floor
/// of the protected tier of the shaped-buffer cache
/// ([`cosmic::PROTECTED_KEEP_FRAMES`], which each entry extends by its own
/// share of [`RENDERED_RUN_KEEP_SPREAD_MASK`]), and the ceiling the backend's
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

/// Extra frames a shaped buffer keeps past [`RENDERED_RUN_KEEP_FRAMES`],
/// masked out of the run's own key. The window is that floor plus a
/// per-run share of this, not one frame every run shares.
///
/// **A shared deadline makes reclamation bursty.** A page switch shapes
/// and promotes a few hundred runs on one frame, so every one of them
/// falls due together 120 frames later. Past the shaped-buffer cache's
/// recycle pool each of those drops frees cosmic's
/// per-line, per-shape and per-layout allocations rather than pooling the
/// buffer, so one frame pays for what a whole navigation created while
/// the frames either side of it pay nothing. Sixteen deadlines instead of
/// one makes that cost proportional to time, which is what the crate asks
/// of anything on the frame path.
///
/// **Masked from the key rather than counted**, because the offset has to
/// be stable per entry. A rotating counter would let a re-promotion move
/// a deadline *inward*, and the expiry wheel's contract is that an inward
/// move owes a fresh ticket. Derived from the key — see
/// [`TextShapeKey::keep_spread`](key::TextShapeKey::keep_spread) — it
/// cannot: the offset is the same every time, so a later frame is always
/// a later deadline.
///
/// Sixteen costs a 256-bucket wheel rather than 128 — about 3 KB of
/// bucket headers, since a ring is sized from the longest deadline its
/// owner hands out.
///
/// Lives here rather than with the cache for the reason
/// [`RENDERED_RUN_KEEP_FRAMES`] gives: the renderer's own suite has to
/// name the window it waits out, and `cosmic` is private to this module.
pub(crate) const RENDERED_RUN_KEEP_SPREAD_MASK: u64 = 15;

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

/// Gated on the feature alone rather than on `any(test, ..)` like its
/// siblings: the one consumer is the allocation suite's scale ramp, which
/// needs a device and so exists only under the feature. Wider is dead
/// code in a plain `cargo test` build, and `-D dead_code` says so.
#[cfg(feature = "internals")]
pub(crate) mod internals {
    /// The raster-scale quantum, so the allocation suite's scale ramp can
    /// step exactly one rung a frame. A ramp that spelled the number
    /// itself would stop minting fresh raster keys the moment this moved,
    /// and a gate that stops missing stops measuring.
    pub const TEXT_SCALE_STEP: f32 = crate::text::TEXT_SCALE_STEP;
}

#[cfg(test)]
mod tests;
