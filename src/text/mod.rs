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
//!   reports no [`key::TextShapeKey`] for those runs and the renderer
//!   drops them. It replaces the *measurement*, not the font system —
//!   a mono shaper holds the same database as any other — so a layout
//!   case states the width it expects as arithmetic instead of as
//!   whatever the bundled face advances to.
//!
//! There's no `TextMeasure` trait: the render path needs `CosmicMeasure`'s
//! shaped buffers + font system (leased cosmic-free through
//! [`render`]), which the mono fallback cannot provide,
//! so a trait would just be a downcast in disguise.
//!
//! # Module layout
//!
//! **Owner modules**, one type each with its private helpers: [`shaper`]
//! the app-global coordinator, [`system`] the per-window reuse slots,
//! [`request`] what a shaping call is asked, [`root`] what an unbounded
//! shape answers, [`key`] the quantized cache identity, [`shaped_ref`]
//! the render handoff, [`run`] how a caller describes a run to probe,
//! [`probe`] the public geometry surface over the lease it holds,
//! [`glyphs`] the render-side lease itself — held by the wgpu backend and
//! by a `GpuView` drawing its own text, which want the same answers off
//! the same borrow. [`font_scope`] names which faces a database starts
//! with, and [`font_scan`] is one of those built on another thread.
//!
//! **Vocabulary modules**, the small value types one layer speaks in and
//! would only scatter with a file apiece: this file (the constants the
//! renderer has to agree with), [`wrap`] the wrap policies, [`render`]
//! the cosmic-free render terms. The three face axes get a file each —
//! [`font_family`], [`font_weight`], [`font_style`] — because each owns a
//! name table, a range check or a tag that is nobody else's business.
//! [`font_source`] is what a registration hands over and [`error`] what
//! it can fail with.
//!
//! **A directory**, for one type with separable jobs: [`cosmic`] is
//! `CosmicMeasure` — the font database, wrapped shaping and truncation —
//! in `mod.rs`, with `shaped_buffer_cache` (what is resident, for how
//! long, and the frame clock that answers both), `cache_entry` (one
//! resident shaped buffer), `cluster_glyph` (the cluster-precise cut's
//! glyph view and prefix scan), `ellipsis_memo` (the reshaped "…"
//! advance), `geometry` (reading measurements back off a shaped buffer),
//! and `counters` beside it. Its children reach the measurer's private
//! fields directly — privacy descends — so the split costs no widening,
//! and each file is one answerable question.

#[cfg(feature = "bench")]
pub(crate) mod bench;
// Private on purpose: no cosmic type is nameable outside `crate::text`, and
// this declaration is what enforces it. `pub(crate)` inside the directory
// therefore means "as far as the ladder reaches without `pub(in path)`" —
// `crate::text`, not the crate — and is what a consumer in a sibling of
// `cosmic` needs, since `pub(super)` there stops at `cosmic` itself.
mod cosmic;
pub(crate) mod error;
pub(crate) mod font_family;
// Gated with its only consumer, the winit host: a build with no windowed
// host has nothing to overlap a font scan with, and `-W dead_code` says so.
#[cfg(feature = "winit")]
pub(crate) mod font_scan;
pub(crate) mod font_scope;
pub(crate) mod font_source;
pub(crate) mod font_style;
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
/// of the protected tier of the shaped-buffer cache, which each entry
/// extends by its own share of [`RENDERED_RUN_KEEP_SPREAD_MASK`], and
/// the ceiling the backend's glyph-template window
/// (`renderer::backend::text::encode::ENCODED_CACHE_KEEP_FRAMES`) must
/// stay under.
///
/// **The relation between the two windows is an ordering, not an
/// equality.** The encoded cache is what generates the render-side
/// buffer lookups, so a buffer whose window expired first would be
/// silently restored from source on a miss the encoder had already
/// counted as cheap — still *correct*, just quietly paying to reshape.
/// Keeping the buffer window the longer one preserves everything that
/// matters, and the `const _` assertion beside the encoded constant is
/// the tripwire. Sharing one value instead would conflate "cannot
/// cross" with "must match", and the encoded side has its own reason to
/// sit well below: its window doubles as its population multiplier,
/// which that constant carries.
///
/// **Comparable numbers are only half of an ordering.** Both windows
/// count frames off **one** clock — the shaped-buffer cache's own
/// — which is what makes comparing them mean anything. Counting off
/// per-cache events instead (this side on the record path, the encoder's
/// on the submit path, which also runs for `PaintOnly` frames) puts the
/// two windows on unequal clocks, and the ordering stops holding: a
/// recorded frame that drew no text would age buffers but not encoded
/// entries.
///
/// **A frame here is a window's, not the host's.** Two windows painting
/// together spend this window in half the host frames it names. The
/// ordering above is unaffected — every reader shrinks together — so
/// read the number as "frames of paint", not as wall time. The clock's
/// own field documents the limit and what closing it would take.
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
