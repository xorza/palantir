//! Real text shaping via [`cosmic_text`]. Caches one shaped `Buffer`
//! per [`TextShapeKey`] — every input that affects shaping (text hash,
//! font size, wrap width, line height, family, weight, halign, fit) —
//! so steady-state measurement is `HashMap` lookup only: no reshape,
//! no allocation. The cache is bounded by **age, not capacity** —
//! [`CosmicMeasure::end_frame`] drops entries untouched for
//! [`PROBATION_KEEP_FRAMES`] / [`PROTECTED_KEEP_FRAMES`], see there for
//! why the two windows exist. Missing buffers are reconstructible from
//! the retained text source at the backend boundary, so a continuous
//! resize drag — every width unique, a fresh entry per run per frame —
//! stays bounded without explicit cache ownership. Evicted buffers feed a
//! bounded recycle pool so later misses retain Cosmic Text's internal
//! line, shaping, and layout allocations. The sweep itself is driven by
//! a deadline wheel, so it costs what expires rather than what is
//! resident — see [`CosmicMeasure::expiry`].
//!
//! The render side never sees cosmic types: `TextShaper::glyphs`
//! lends a `RefMut<CosmicMeasure>` whose
//! [`CosmicMeasure::extract_glyphs`] / [`CosmicMeasure::rasterize_glyph`]
//! translate shaped buffers into palantir-native placements and bitmaps;
//! [`crate::text`] documents why there's no `TextMeasure` trait.
//!
//! Hash collisions are theoretically possible (we key on a 64-bit hash of the
//! text rather than storing the full string), but at typical UI scales the
//! cost of resolving them — verifying with the cached buffer's source string
//! on every hit — outweighs the cost of accepting the negligible risk.

use crate::common::expiry_wheel::ExpiryWheel;
use crate::layout::types::align::HAlign;
use crate::text::cosmic::counters::CacheCounters;
use crate::text::key::TextShapeKey;
use crate::text::request::TextShapeRequest;
use crate::text::root::TextRoot;
use crate::text::wrap::{LineFit, WrapFloor};
use crate::text::{FontFamily, FontWeight};
use cosmic_text::{
    Align as CosmicAlign, Attrs, Buffer, CacheKeyFlags, Family, FontSystem, Metrics, Shaping,
    SwashCache, Weight, fontdb,
};
use rustc_hash::FxHashMap;
use std::sync::Arc;
use tinyvec::ArrayVec;

use crate::primitives::num::F32Ext;
use crate::text::cosmic::cache_entry::CacheEntry;
use crate::text::cosmic::geometry::{ShapedGeometry, intrinsic_min_width, shaped_geometry};
use crate::text::cosmic::truncate::{
    ClusterGlyph, EllipsisMemo, first_line_right, fitting_prefix, truncation_probe,
};
use crate::text::render::{
    GlyphImage, GlyphImageKind, GlyphPlacement, GlyphRasterKey, PlacedGlyph, RunPlacement,
};
use cosmic_text::SwashContent;
use std::collections::hash_map::Entry;

pub(super) mod cache_entry;
pub(super) mod counters;
pub(super) mod geometry;
pub(super) mod truncate;

/// Bundled fonts shipped with the crate. Inter is the default UI /
/// proportional body font; JetBrains Mono is the monospace. Both ship as
/// a single variable-weight (`wght`) face, so Regular and Bold come from
/// one file each. Both are OFL 1.1. Weight is selected per-run via
/// [`FontWeight`] on the [`crate::TextStyle`], resolved in [`attrs_for`].
const INTER: &[u8] = include_bytes!("../../../assets/fonts/Inter-VariableFont_opsz,wght.ttf");
const JBMONO: &[u8] = include_bytes!("../../../assets/fonts/JetBrainsMono[wght].ttf");

const RECYCLE_POOL_CAP: usize = 128;

/// Faces [`CosmicMeasure::ellipsis`] remembers the "…" advance for.
const ELLIPSIS_MEMO_SLOTS: usize = 4;

/// Frames a *probationary* entry survives before
/// [`CosmicMeasure::end_frame`] drops it: one inserted and never looked
/// up, or one [superseded](CosmicMeasure::supersede) after its reuse
/// slot moved to a different key.
///
/// Short on purpose. This population is scan traffic: a resize or zoom
/// drag quantizes to a new whole-pixel wrap width nearly every frame, so
/// each run mints a key that will never be asked for again. Holding those
/// for the protected window lets one drag accumulate
/// `runs × PROTECTED_KEEP_FRAMES` dead buffers.
///
/// **Supersession is what makes this window reach that population.**
/// Insertion alone does not: layout shapes a run and the encoder renders
/// it on the *same* frame, and that render is a lookup, so every drawn
/// buffer would otherwise be promoted the moment it was created and the
/// probation tier would be inert. Steady state cannot repair that by
/// re-touching it either — the measure cache and the encoded-run cache
/// both short-circuit before reaching here, so a resident buffer is
/// never looked up again on a later frame. `TextSystem` holds the only
/// signal that distinguishes "this run wants a different shape now"
/// (drag, typing, animation — dead) from "this run left the tree"
/// (scrolled away — may well return), and it reports the first through
/// [`CosmicMeasure::supersede`].
///
/// A demotion, not an eviction: four frames of grace means a label
/// oscillating between two keys, or a drag reversing back through a
/// width it just used, still hits.
///
/// # Why not reference-counted retention
///
/// Letting an entry die when no upper cache still holds it reads like
/// the obvious replacement for this whole scheme — no windows, no
/// demotion signal. It does not work, for a measured reason.
/// `EncodedKey` embeds [`TextShapeKey`], so a width drag mints a fresh
/// encoded entry every frame and those live
/// `ENCODED_CACHE_KEEP_FRAMES` — a shorter window than this one, but
/// still one with no probation tier under it. An encoded entry holding
/// its buffer *strongly* therefore pins `runs × (that window + 1)` of
/// them, an order of magnitude past what this window achieves, and the
/// exact growth it was added to stop. Holding it *weakly* keeps the drag
/// bounded but leaves buffers dying under live encoded entries, so the
/// whole restore path (`ShapedTextRef`, `TextSource`,
/// [`CosmicMeasure::ensure_buffer`]) has to stay — and deleting that was
/// the other half of the idea. The two wins are mutually exclusive.
pub(super) const PROBATION_KEEP_FRAMES: u64 = 4;

/// Frames a *protected* entry — one that has been looked up at least once
/// since insertion — survives untouched.
///
/// The name cosmic's two-tier policy reads
/// [`crate::text::RENDERED_RUN_KEEP_FRAMES`] under, beside
/// [`PROBATION_KEEP_FRAMES`]. Why that number is a ceiling the encoded
/// cache stays under rather than one it shares is stated there.
pub(super) const PROTECTED_KEEP_FRAMES: u64 = crate::text::RENDERED_RUN_KEEP_FRAMES;

fn recycle_buffer(pool: &mut Vec<Buffer>, buffer: Buffer) {
    if pool.len() < RECYCLE_POOL_CAP {
        pool.push(buffer);
    }
}

fn attrs_for(family: FontFamily, weight: FontWeight) -> Attrs<'static> {
    // Skip TrueType bytecode hinting: skrifa's hint VM dominated zoom-frame
    // CPU time, and at HiDPI / during animated zoom the visual difference
    // is imperceptible.
    let base = Attrs::new().cache_key_flags(CacheKeyFlags::DISABLE_HINTING);
    let base = match weight {
        // `Weight::NORMAL` is fontdb's default; requesting Bold makes
        // fontdb instantiate the `wght` axis at 700 on the variable face
        // (both Inter and JetBrains Mono ship as single variable fonts).
        FontWeight::Regular => base,
        FontWeight::Bold => base.weight(Weight::BOLD),
    };
    match family {
        FontFamily::Mono => base.family(Family::Name("JetBrains Mono")),
        FontFamily::Sans => base.family(Family::Name("Inter")),
    }
}

/// Map an Palantir [`HAlign`] to cosmic-text's per-line align.
/// `Auto`/`Stretch` map to `None` — cosmic falls back to its
/// left-or-rtl-aware default, identical bit-for-bit to the legacy
/// "no per-line align" path. `Left`/`Center`/`Right` translate
/// directly. Cosmic's `Justified` and `End` aren't surfaced.
fn cosmic_align(halign: HAlign) -> Option<CosmicAlign> {
    match halign {
        HAlign::Left => Some(CosmicAlign::Left),
        HAlign::Center => Some(CosmicAlign::Center),
        HAlign::Right => Some(CosmicAlign::Right),
        // `Auto` is the documented "no per-line align" default;
        // `Stretch` doesn't make sense per-line for text and falls
        // through to the same path.
        HAlign::Auto | HAlign::Stretch => None,
    }
}

/// A resident shaped buffer paired with the x its glyph block starts at,
/// so every reader normalizes the same way off one lookup.
#[derive(Clone, Copy, Debug)]
pub(super) struct ShapedRun<'a> {
    pub(super) buffer: &'a Buffer,
    /// See [`CacheEntry::left`].
    pub(super) left: f32,
}

/// Real-shaping text measurer. Owns a [`FontSystem`] populated by
/// [`CosmicMeasure::with_bundled_fonts`] (Inter + JetBrains Mono) and
/// a cache of shaped `Buffer`s keyed on the inputs that affect shaping.
/// Per-call font family + weight selection comes from [`FontFamily`] /
/// [`FontWeight`] on each measurement; internal named lookups resolve against
/// the bundled set.
pub(super) struct CosmicMeasure {
    font_system: FontSystem,
    /// Swash rasterization context for [`Self::rasterize_glyph`]. Used
    /// uncached — the renderer's glyph atlas is the real bitmap cache.
    swash_cache: SwashCache,
    cache: FxHashMap<TextShapeKey, CacheEntry>,
    /// Latest value of the shaper's shared frame clock, mirrored here by
    /// [`Self::end_frame`]. Stamped onto every entry this touches, and
    /// the reference point both retention windows measure back from.
    ///
    /// Mirrored rather than counted: the renderer's encoded-run cache
    /// and glyph atlas age against the same clock, and only one owner
    /// can keep them in step — see
    /// [`ShaperInner::frame`](crate::text::shaper::ShaperInner).
    frame: u64,
    /// Which keys come due on which frame, so [`Self::end_frame`] costs
    /// what expires rather than what is resident.
    ///
    /// The earlier design kept the earliest `keep_until` in the cache and
    /// skipped the sweep while nothing could have expired. That is O(1)
    /// only while nothing churns: a single key that changes every frame —
    /// a clock, an FPS counter, a scrubbing value — re-pins that minimum
    /// one probation window out on every insert, the gate stops firing,
    /// and every frame walks the whole map to reclaim one entry. The
    /// churn it was measured against is precisely the churn that defeats
    /// it.
    expiry: ExpiryWheel<TextShapeKey>,
    /// LIFO pool fed by LRU eviction. `Buffer::set_text` reclaims its
    /// line, shaping, and layout allocations when the buffer is reset.
    recycle_pool: Vec<Buffer>,
    /// Trailing advance of "…" for the last few faces asked about.
    ///
    /// A fixed set of slots, not a map: a frame draws its ellipsized
    /// labels in a handful of text styles, and a miss is a single glyph
    /// through a recycled buffer. Nothing to bound, evict, or clear —
    /// the round-robin victim is simply overwritten.
    ///
    /// One slot was not enough. It held only the *last* face, so any
    /// record order that interleaves two — header and detail rows, a
    /// tree sized per depth, regular beside bold in one row — missed on
    /// every single truncation. Measured on
    /// `text_shape/ellipsis_width_churn`: the `two_faces` arm runs
    /// 3.85 µs on one slot against 2.77 µs on four, a 28% cut, while
    /// `one_face` is unchanged inside noise — so the extra slots cost
    /// nothing in the easy case. Four covers the interleavings a frame
    /// actually produces, and a lookup is four compares against a
    /// `Copy` struct.
    ///
    /// Newest first: a miss pushes to the front and drops the back, so
    /// the entry evicted is the one shaped longest ago and the linear
    /// scan meets the most recently shaped face first. With four
    /// entries the shift is three 12-byte copies, cheaper than the
    /// cursor an in-place ring would need.
    ellipsis: ArrayVec<[EllipsisMemo; ELLIPSIS_MEMO_SLOTS]>,
    /// Retained scratch for the truncated string
    /// [`Self::measure_truncated`] builds on a miss (cut prefix +
    /// optional `…`). Misses are the hot case — a continuous width drag
    /// mints a fresh quantized target per label per frame — so building
    /// into a retained buffer keeps that path free of `String` allocs,
    /// while the unbounded probe itself comes from `cache`.
    truncate_scratch: String,
    /// Retained scratch for `collect_break_offsets`, so the unbounded
    /// shape's segment scan allocates nothing per miss.
    break_scratch: Vec<u32>,
    /// Retained scratch holding the truncation probe's glyph indices in
    /// logical order — visual order is what the shaped run gives us, and
    /// truncation needs the logical prefix.
    logical_order: Vec<u32>,
    /// Shape / hit / supersede / expire tallies. Zero-sized outside
    /// tests.
    pub(super) counters: CacheCounters,
}

impl CosmicMeasure {
    /// Register the bundled faces — the variable-weight Inter (the default
    /// proportional family) and the variable-weight JetBrains Mono
    /// (monospace) — so they're always resolvable by name + weight.
    /// cosmic-text's `new_with_fonts` *also* loads the platform's system
    /// fonts, which act as glyph fallback for scripts the bundled faces
    /// don't cover — so text metrics are *not* guaranteed identical
    /// across machines. Each measurement selects its [`FontFamily`] and
    /// [`FontWeight`].
    pub(super) fn with_bundled_fonts() -> Self {
        let sources = [INTER, JBMONO]
            .into_iter()
            .map(|b| fontdb::Source::Binary(Arc::new(b)));
        let font_system = FontSystem::new_with_fonts(sources);
        Self {
            font_system,
            swash_cache: SwashCache::new(),
            cache: FxHashMap::default(),
            frame: 0,
            expiry: ExpiryWheel::with_horizon(PROTECTED_KEEP_FRAMES + 2),
            recycle_pool: Vec::with_capacity(RECYCLE_POOL_CAP),
            ellipsis: ArrayVec::new(),
            truncate_scratch: String::new(),
            break_scratch: Vec::new(),
            logical_order: Vec::new(),
            counters: CacheCounters::default(),
        }
    }

    /// Look up the shaped run for `key`, or `None` when no buffer is
    /// resident under it — never measured on this `CosmicMeasure`, or
    /// aged out since.
    ///
    /// Unlike [`Self::ensure_buffer`] this is a lookup, so absence is an
    /// answer rather than a wiring bug: the probe path takes it for a run
    /// that was never shaped, and a residency check is the question
    /// itself.
    ///
    /// [`TextShapeKey::INVALID`] is not among the keys it answers for.
    /// The sentinel means "this run has no shaped buffer at all", which
    /// every caller knows before it gets here — the encoder drops those
    /// runs — and nothing ever inserts one, since [`Self::insert`] is
    /// reached only through [`Self::shape`], which returns before shaping
    /// empty text. Asking the cache about it is a category error rather
    /// than a miss, so it is asserted instead of answered.
    pub(super) fn shaped_run(&self, key: TextShapeKey) -> Option<ShapedRun<'_>> {
        debug_assert!(
            !key.is_invalid(),
            "the invalid sentinel names no cache entry — filter it before the lookup",
        );
        self.cache.get(&key).map(|e| ShapedRun {
            buffer: &e.buffer,
            left: e.left,
        })
    }

    /// Shape `request`, routing it to the wrapping or truncating path.
    ///
    /// Empty text answers here rather than in either path: it mints no
    /// buffer, which is the contract [`TextShapeKey::INVALID`] pairs
    /// with, and both `ensure_buffer` and the gated test helpers enter
    /// through this function.
    pub(super) fn shape(&mut self, request: TextShapeRequest<'_>, floor: WrapFloor) -> TextRoot {
        if request.text.is_empty() {
            return TextRoot::ZERO;
        }
        match (request.key.fit(), request.key.max_width_px()) {
            (LineFit::Clip | LineFit::Ellipsis, Some(_)) => self.measure_truncated(request),
            _ => self.measure_wrapped(request, floor),
        }
    }

    fn measure_wrapped(&mut self, request: TextShapeRequest<'_>, floor: WrapFloor) -> TextRoot {
        debug_assert!(
            floor == WrapFloor::Skip || request.key.max_width_px().is_none(),
            "the wrap floor is a property of the unbounded root",
        );
        let key = request.key;
        if let Some(hit) = self.cache_hit(key) {
            // A resident entry shaped by a policy that didn't want the
            // floor still owes it to one that does.
            if floor == WrapFloor::Scan && hit.intrinsic_min.is_none() {
                return TextRoot {
                    intrinsic_min: Some(self.scan_wrap_floor(key)),
                    ..hit
                };
            }
            return hit;
        }

        let metrics = Metrics::new(key.font_size_px(), key.line_height_px());
        let mut buffer = self.acquire_buffer(metrics, key.max_width_px());
        // Per-line alignment travels through cosmic's `set_text`
        // `alignment` slot — that's the canonical entry point and
        // applies the align to every parsed buffer line in one
        // shot. Iterating `buffer.lines.iter_mut().set_align` after
        // `set_text` is the older API surface and tends to no-op on
        // freshly populated lines in 0.18+. Per-line align is only
        // meaningful with a finite wrap target (cosmic uses it as the
        // line width); without one we pass `None` so single-line
        // editors keep their widget-side `dx` placement.
        let alignment = key.max_width_px().and_then(|_| cosmic_align(key.halign()));
        buffer.set_text(
            request.text,
            &attrs_for(key.family(), key.weight()),
            Shaping::Advanced,
            alignment,
        );
        buffer.shape_until_scroll(&mut self.font_system, false);

        let geometry = shaped_geometry(&buffer, floor, &mut self.break_scratch);
        self.insert(key, buffer, geometry);
        geometry.root
    }

    /// Restore a missing shaped buffer from the retained source text and
    /// the canonical parameters encoded by `key`, and hand it back.
    /// Truncated runs restore their unbounded probe first; callers never
    /// manage that dependency. Any valid key is shaped on the spot, so
    /// the final lookup doubles as the check that the restore landed
    /// under its own key.
    ///
    /// A run with no shaped buffer never gets this far: the encoder drops
    /// [`TextShapeKey::INVALID`] runs before paint and the backend keeps
    /// its own backstop, so arriving with one is a wiring bug rather than
    /// a case to answer. Handing back an `Option` for it only moved the
    /// panic to the caller, which is where it used to live.
    pub(super) fn ensure_buffer(&mut self, request: TextShapeRequest<'_>) -> ShapedRun<'_> {
        debug_assert!(
            !request.key.is_invalid(),
            "restoring a buffer for a run the encoder should have dropped",
        );
        if self.cache_hit(request.key).is_none() {
            self.shape(request, WrapFloor::Skip);
        }
        self.shaped_run(request.key)
            .expect("restored text buffer did not land under its own TextShapeKey")
    }

    /// Scan the wrap floor for a resident entry that was shaped without
    /// one, memoizing it so the next asker pays nothing.
    ///
    /// Disjoint field borrows: the buffer comes out of `cache` while
    /// `break_scratch` is borrowed alongside it, which only holds written
    /// out here rather than behind a `&mut self` helper.
    fn scan_wrap_floor(&mut self, key: TextShapeKey) -> f32 {
        let breaks = &mut self.break_scratch;
        let entry = self
            .cache
            .get_mut(&key)
            .expect("a cache hit must still be resident");
        if let Some(floor) = entry.root.intrinsic_min {
            return floor;
        }
        let floor = intrinsic_min_width(&entry.buffer, breaks);
        entry.root.intrinsic_min = Some(floor);
        floor
    }

    fn acquire_buffer(&mut self, metrics: Metrics, width: Option<f32>) -> Buffer {
        let mut buffer = match self.recycle_pool.pop() {
            Some(buffer) => buffer,
            None => Buffer::new(&mut self.font_system, metrics),
        };
        buffer.set_metrics_and_size(metrics, width, None);
        buffer
    }

    // ---- shaped-buffer cache retention ----

    /// Store a freshly shaped buffer. Entries start probationary; only a
    /// later lookup promotes them (see [`PROBATION_KEEP_FRAMES`]).
    fn insert(&mut self, key: TextShapeKey, buffer: Buffer, geometry: ShapedGeometry) {
        // Counted here rather than per `shape_until_scroll` so one
        // cached run is one tally: `measure_truncated`'s back-off can
        // reshape a prefix several times to land inside the committed
        // width, and a workload test cares that the run was shaped, not
        // how many attempts the cut took. The memoized ellipsis probe
        // shapes without inserting and is deliberately not counted.
        self.counters.shapes.bump();
        let keep_until = self.frame + PROBATION_KEEP_FRAMES;
        // First frame on which the entry is dead, matching the sweep's
        // own `keep_until < frame` test.
        let ticket_seq = self.expiry.schedule(key, keep_until + 1);
        let displaced = self.cache.insert(
            key,
            CacheEntry {
                buffer,
                root: geometry.root,
                left: geometry.left,
                keep_until,
                ticket_seq,
            },
        );
        // Unreachable today — every caller checks `cache_hit` first, so a
        // key is inserted once — but the pool must not silently leak a
        // buffer the moment a second insert path appears.
        if let Some(old) = displaced {
            recycle_buffer(&mut self.recycle_pool, old.buffer);
        }
    }

    /// A cached entry's [`TextRoot`] for `key`, or `None` on a miss.
    /// Layout hits and encoder ensures both land here, and both push the
    /// entry's deadline out to the protected window — being asked for at
    /// all is the evidence that separates reuse from scan traffic, so no
    /// separate promotion step is needed.
    pub(super) fn cache_hit(&mut self, key: TextShapeKey) -> Option<TextRoot> {
        let keep_until = self.frame + PROTECTED_KEEP_FRAMES;
        let hit = self.cache.get_mut(&key).map(|entry| {
            entry.keep_until = keep_until;
            entry.root
        });
        if hit.is_some() {
            self.counters.hits.bump();
        }
        hit
    }

    /// Demote `key` to the probation window: the reuse slot that owned
    /// it now answers a different key, so nothing can ask for it through
    /// that slot again. See [`PROBATION_KEEP_FRAMES`] for why this is
    /// the signal the two-tier policy runs on.
    ///
    /// Only ever shortens a deadline — a supersede must not extend the
    /// life of an entry already closer to expiry — and files a second
    /// ticket for the earlier frame, since the outstanding one sits at
    /// the deadline this just retracted.
    ///
    /// Silent on a key that isn't resident: the buffer may already have
    /// aged out, and superseding what is gone is a no-op, not an error.
    pub(crate) fn supersede(&mut self, key: TextShapeKey) {
        if key.is_invalid() {
            return;
        }
        let keep_until = self.frame + PROBATION_KEEP_FRAMES;
        let Some(entry) = self.cache.get_mut(&key) else {
            return;
        };
        self.counters.supersedes.bump();
        // Never *extends* a life: an entry already closer to expiry —
        // one that was inserted and never looked up — keeps its own
        // deadline.
        if entry.keep_until > keep_until {
            entry.keep_until = keep_until;
            // The new ticket is earlier than the outstanding one, so it
            // is the one that decides this entry's fate: stamping it
            // here retires the supplanted ticket when it fires.
            entry.ticket_seq = self.expiry.schedule(key, keep_until + 1);
        }
    }

    /// Take the shaper's `frame` clock and drop every buffer whose
    /// deadline has passed.
    ///
    /// `frame` is only ever read, never derived from a local counter, so
    /// a clock that jumps several frames at once is handled by the same
    /// comparison as one that advances by one.
    ///
    /// Age, not capacity. A count budget cannot express what this cache
    /// needs: set below the live working set it thrashes — UI redraw is a
    /// cyclic access pattern, LRU's worst case, so the overflow misses
    /// every frame forever — and set above it, a resize drag fills it with
    /// widths that can never be hit again. Ageing bounds both without a
    /// number to guess: an app keeps exactly what it keeps touching, and
    /// scan traffic falls out on its own.
    ///
    /// Cost tracks what expires, not what is resident: [`Self::expiry`]
    /// hands back only the keys whose ticket came due, so a frame holding
    /// a scrolled document's whole working set pays the same as an empty
    /// one unless something actually lapsed.
    ///
    /// A ticket is a hint, never authority to drop. Deadlines move after
    /// it is filed — [`Self::cache_hit`] pushes one out and deliberately
    /// files nothing, which is what keeps a re-read entry from filing a
    /// ticket per frame — so the real `keep_until` is re-read here and a
    /// still-live entry is simply re-filed.
    pub(crate) fn end_frame(&mut self, frame: u64) {
        debug_assert!(frame >= self.frame, "the shared frame clock ran backwards");
        self.frame = frame;
        let cache = &mut self.cache;
        let recycle_pool = &mut self.recycle_pool;
        let probe = &mut self.counters;
        self.expiry.retire(frame, |key, seq| {
            // Retired already — a demote leaves two tickets outstanding
            // and both can come due in one drain, so whichever settled
            // first may have evicted the entry this one is holding.
            let Entry::Occupied(slot) = cache.entry(key) else {
                return None;
            };
            // Supplanted by a later `supersede`: the entry's live ticket
            // is still outstanding and will settle it, so this one is
            // surplus and dies here. Re-filing it instead is what let the
            // per-entry ticket count — and with it the per-frame drain —
            // grow for as long as the entry stayed resident.
            if seq != slot.get().ticket_seq {
                return None;
            }
            if slot.get().keep_until >= frame {
                // Re-filed under the same serial, so the entry's stamp
                // still names it and nothing has to be written back.
                return Some(slot.get().keep_until + 1);
            }
            probe.expiries.bump();
            recycle_buffer(recycle_pool, slot.remove().buffer);
            None
        });
    }

    // ---- render-side glyph resolution ----

    /// Resolve `request` to palantir-native glyph placements for the
    /// renderer. Restores the shaped buffer if evicted (truncated runs
    /// restore their unbounded probe internally), walks its layout runs,
    /// y-culls whole lines against `placement.bounds`, and rewrites
    /// `out` with one [`PlacedGlyph`] per surviving glyph. Returns
    /// whether any line was culled — such partial extractions must not
    /// become renderer cache templates (its encoded key carries no
    /// bounds).
    pub(crate) fn extract_glyphs(
        &mut self,
        request: TextShapeRequest<'_>,
        placement: RunPlacement,
        out: &mut Vec<PlacedGlyph>,
    ) -> bool {
        let ShapedRun { buffer, left } = self.ensure_buffer(request);

        out.clear();
        let RunPlacement {
            origin,
            scale,
            bounds,
        } = placement;
        // `origin` positions the *measured block*, whose left edge is
        // `left` in buffer space — so pull the origin back by it and the
        // per-glyph offsets land where the measurement said they would.
        // Folding it into the origin rather than into each `physical.x`
        // keeps the subpixel binning consistent with the shift.
        let origin_x = origin.x - left * scale;
        let bounds_top = bounds.min.y as f32;
        let bounds_bot = bounds.max().y as f32;
        let mut culled = false;
        for run in buffer.layout_runs() {
            if (run.line_top + run.line_height) * scale + origin.y < bounds_top {
                culled = true;
                continue;
            }
            if run.line_top * scale + origin.y > bounds_bot {
                culled = true;
                break;
            }
            let line_y_px = (run.line_y * scale).fast_round() as i32;
            for glyph in run.glyphs.iter() {
                // The renderer caches encoded runs on one uniform area
                // colour — correct only while cosmic never produces a
                // per-glyph override ([`attrs_for`] sets no per-span
                // colour). If this fires, per-span colour was added
                // without folding a colour fingerprint into the
                // renderer's `EncodedKey`.
                debug_assert!(
                    glyph.color_opt.is_none(),
                    "per-glyph colour override requires folding colour into EncodedKey",
                );
                let physical = glyph.physical((origin_x, origin.y), scale);
                out.push(PlacedGlyph {
                    raster_key: GlyphRasterKey(physical.cache_key),
                    x: physical.x,
                    y: line_y_px + physical.y,
                });
            }
        }
        culled
    }

    /// Rasterize one glyph via swash, uncached on the cosmic side — the
    /// renderer's atlas is the real cache. `None` when swash cannot
    /// produce an image for the key (e.g. a glyph the face lacks).
    pub(crate) fn rasterize_glyph(&mut self, key: GlyphRasterKey) -> Option<GlyphImage> {
        let image = self
            .swash_cache
            .get_image_uncached(&mut self.font_system, key.0)?;
        let kind = match image.content {
            SwashContent::Color => GlyphImageKind::Color,
            SwashContent::Mask | SwashContent::SubpixelMask => GlyphImageKind::Mask,
        };
        Some(GlyphImage {
            kind,
            placement: GlyphPlacement {
                left: image.placement.left,
                top: image.placement.top,
                width: image.placement.width,
                height: image.placement.height,
            },
            data: image.data,
        })
    }

    // ---- cluster-precise truncation ----

    /// Shape `text` as a single line truncated to fit `w`. Truncation is
    /// cluster-precise: the cached unbounded shape gives per-glyph advances,
    /// [`fitting_prefix`] cuts after the last fully paid-for cluster, then we
    /// shape the (possibly truncated) prefix on one **natural** line — no
    /// per-line align. The committed width only decides the cut; the encoder
    /// positions/aligns the single line, so the measured extent is the glyph
    /// width, not `w` (binding to `w` + center align would inflate a
    /// fits-anyway label to ~half the box). `LineFit::Ellipsis` reserves room
    /// for and appends a trailing `…`; `LineFit::Clip` cuts flush to `w`
    /// with no marker. The buffer caches under a fit-discriminated key (so it
    /// can't collide with the wrapped buffer — or the other truncation mode —
    /// at the same width). `intrinsic_min` is 0 — a truncated run can shrink
    /// to nothing.
    ///
    /// The shaped prefix is verified against `w` and retires a further
    /// cluster until it fits, so the measured extent never exceeds the
    /// committed width — the cut alone cannot guarantee that, since
    /// reshaping the prefix changes its shaping context.
    ///
    /// # Why not `Buffer::set_ellipsize`
    ///
    /// Cosmic 0.19 can do this itself, and delegating to it deletes
    /// roughly 290 lines: this function, [`fitting_prefix`],
    /// [`ClusterGlyph`], the ellipsis memo, and three retained scratch
    /// fields. That was written, measured, and reverted — **4.9x slower**
    /// on `text_shape/resize_drag_frame` (1.42 µs -> 7.13 µs per run per
    /// frame).
    ///
    /// The reason is not the wrap mode (`Wrap::Glyph`, `Wrap::None` and a
    /// one-line height cap all measure within 10% of each other) but how
    /// much text is reshaped per committed width. Cosmic has to see the
    /// whole string to decide where to cut, so every new width reshapes
    /// all of it; shaping a 108-character label costs ~9x what its
    /// seven-character cut prefix costs.
    ///
    /// **So the dependency on the cached unbounded probe is the point,
    /// not a wart.** It is what a drag reuses: the full-string shape is
    /// paid once, [`fitting_prefix`] finds the cut by scanning glyphs
    /// that are already there, and only the short prefix is reshaped per
    /// frame. Anything that revisits this has to keep the full-string
    /// shape cached across widths — cosmic allows that (`set_size`
    /// re-lays-out without re-shaping), but one buffer holds one layout
    /// and the cache is keyed per width, so it needs a different
    /// buffer/key model rather than a different call.
    pub(super) fn measure_truncated(&mut self, request: TextShapeRequest<'_>) -> TextRoot {
        let key = request.key;
        let fit = key.fit();
        let width = key
            .max_width_px()
            .expect("measure_truncated requires a finite width");
        debug_assert!(
            matches!(fit, LineFit::Clip | LineFit::Ellipsis),
            "measure_truncated requires Clip or Ellipsis",
        );
        if let Some(hit) = self.cache_hit(key) {
            return hit;
        }
        let unbounded = request.unbounded_version();
        self.ensure_buffer(unbounded);
        let metrics = Metrics::new(key.font_size_px(), key.line_height_px());
        let family = key.family();
        let weight = key.weight();
        let attrs = attrs_for(family, weight);
        // Reserve the ellipsis width only when we'll append one; a plain
        // clip cuts flush to the full available width. Resolved before
        // borrowing the probe, since shaping "…" needs `&mut self`.
        let mut append_ellipsis = false;
        let avail = if matches!(fit, LineFit::Ellipsis) {
            let ellipsis_w = self.ellipsis_advance(key.size_q, metrics, family, weight);
            append_ellipsis = ellipsis_w <= width;
            (width - ellipsis_w).max(0.0)
        } else {
            width
        };
        let probe_key = unbounded.key;
        // Same question `TextSystem::measure` asks before it ever gets
        // here, against the same root — so it is asked the same way. The
        // probe's own measurement already answers it, which is why this
        // reads the entry rather than re-walking its glyphs.
        let fits_whole =
            fit.resolves_to_unbounded(&truncation_probe(&self.cache, probe_key).root, width);

        // Shape unbounded on one line: the cut already fit it to `w`, and the
        // encoder owns single-line placement. Binding to `Some(w)` + align
        // would measure the aligned glyph position, inflating a fits-anyway
        // label toward the box width.
        let mut buffer = self.acquire_buffer(metrics, None);
        let size = if fits_whole {
            // Re-shaping the identical text reproduces the probe, so this
            // branch cannot overrun `width`.
            buffer.set_text(request.text, &attrs, Shaping::Advanced, None);
            buffer.shape_until_scroll(&mut self.font_system, false);
            shaped_geometry(&buffer, WrapFloor::Skip, &mut self.break_scratch)
                .root
                .size
        } else {
            // The cut spends advances measured in the *whole* run's shaping,
            // but the prefix reshapes in its own context: a joining script's
            // last letter is exposed at a new word end and takes a final form
            // wider than the medial one the budget paid for. So verify the
            // shaped result, and while it overruns, retire one more cluster.
            // `max_end` makes every retry strictly shorter, so the sequence
            // bottoms out at the empty prefix.
            let mut max_end = usize::MAX;
            loop {
                let cut = match truncation_probe(&self.cache, probe_key)
                    .buffer
                    .layout_runs()
                    .next()
                {
                    Some(run) => fitting_prefix(
                        run.glyphs.len(),
                        |i| ClusterGlyph {
                            start: run.glyphs[i].start,
                            end: run.glyphs[i].end,
                            advance: run.glyphs[i].w,
                        },
                        &mut self.logical_order,
                        avail,
                        max_end,
                    ),
                    None => 0,
                };
                self.truncate_scratch.clear();
                self.truncate_scratch
                    .push_str(request.text[..cut].trim_end());
                if append_ellipsis {
                    self.truncate_scratch.push('…');
                }
                // `set_text` resets the buffer in place, so a retry reuses
                // the line, shaping, and layout allocations it just filled.
                buffer.set_text(
                    self.truncate_scratch.as_str(),
                    &attrs,
                    Shaping::Advanced,
                    None,
                );
                buffer.shape_until_scroll(&mut self.font_system, false);
                let size = shaped_geometry(&buffer, WrapFloor::Skip, &mut self.break_scratch)
                    .root
                    .size;
                if size.w <= width || cut == 0 {
                    break size;
                }
                max_end = cut;
            }
        };

        // Truncated runs are one natural line by construction: the cut
        // prefix comes from the unbounded probe's first layout run, and a
        // truncated run can shrink to nothing, so its floor is zero.
        // The prefix reshapes on an unbounded buffer with no per-line
        // align, so its block already starts at 0.
        let geometry = ShapedGeometry {
            root: TextRoot {
                size,
                // Genuinely zero, not unscanned: a truncated run can
                // shrink to nothing.
                intrinsic_min: Some(0.0),
                single_line: true,
            },
            left: 0.0,
        };
        self.insert(key, buffer, geometry);
        geometry.root
    }

    /// Trailing advance of "…" at `metrics`/`family`/`weight`, memoized for
    /// the last face asked about.
    ///
    /// Only the *opening* budget: [`Self::measure_truncated`] verifies the
    /// shaped result against the committed width either way, so a stale or
    /// imprecise reservation costs retries, never correctness. What it buys
    /// is measured on `text_shape/ellipsis_width_churn`, whose arms hold
    /// the width churning so every frame is a truncation miss and the
    /// reservation is asked for again. See [`CosmicMeasure::ellipsis`]
    /// for what the slot count buys there.
    fn ellipsis_advance(
        &mut self,
        size_q: u32,
        metrics: Metrics,
        family: FontFamily,
        weight: FontWeight,
    ) -> f32 {
        let want = EllipsisMemo::wanted(size_q, family, weight);
        if let Some(advance) = self
            .ellipsis
            .iter()
            .find_map(|memo| memo.advance_for(&want))
        {
            return advance;
        }
        let mut buffer = self.acquire_buffer(metrics, None);
        buffer.set_text("…", &attrs_for(family, weight), Shaping::Advanced, None);
        buffer.shape_until_scroll(&mut self.font_system, false);
        let advance = first_line_right(&buffer);
        recycle_buffer(&mut self.recycle_pool, buffer);
        self.counters.ellipsis_misses.bump();
        // `insert` panics at capacity, so retire the oldest first.
        if self.ellipsis.len() == ELLIPSIS_MEMO_SLOTS {
            self.ellipsis.pop();
        }
        self.ellipsis.insert(0, want.measured(advance));
        advance
    }
}

impl Default for CosmicMeasure {
    fn default() -> Self {
        Self::with_bundled_fonts()
    }
}

// Manual: cosmic's `SwashCache` isn't `Debug`.
impl std::fmt::Debug for CosmicMeasure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CosmicMeasure")
            .field("cache", &self.cache.len())
            .field("frame", &self.frame)
            .finish_non_exhaustive()
    }
}

// Wider than `cfg(test)`: `drop_all_buffers` is reached from the
// `internals`-gated GPU tests in `renderer::backend::text`, which build
// without `cfg(test)`.
#[cfg(any(test, feature = "bench"))]
mod internals {
    use super::*;
    #[cfg(test)]
    use crate::text::request::internals::TestShape;
    #[cfg(test)]
    use crate::text::root::internals::TestMeasure;

    #[derive(Debug, PartialEq, Eq)]
    #[cfg(test)]
    pub(crate) struct RecyclePoolStats {
        pub(crate) len: usize,
        pub(crate) capacity: usize,
        pub(crate) limit: usize,
    }

    impl CosmicMeasure {
        #[cfg(test)]
        pub(crate) fn measure(&mut self, text: &str, shape: TestShape) -> TestMeasure {
            self.measure_with_fit_key(shape.request(text, LineFit::Wrap))
        }

        /// Shape `request` and pair the result with the key it shaped
        /// under — invalid for empty text, which mints no buffer.
        #[cfg(test)]
        fn measure_with_fit_key(&mut self, request: TextShapeRequest<'_>) -> TestMeasure {
            let key = if request.text.is_empty() {
                TextShapeKey::INVALID
            } else {
                request.key
            };
            // The wrap-floor tests measure through this helper, so it
            // asks for the floor on every root it shapes — but the floor
            // is an unbounded-root property, so a bounded request skips
            // it exactly as production does.
            let floor = match request.key.max_width_px() {
                Some(_) => WrapFloor::Skip,
                None => WrapFloor::Scan,
            };
            TestMeasure::new(self.shape(request, floor), key)
        }

        /// Truncating-fit measure. Named apart from the production
        /// `measure_truncated` — inherent methods can't share a name.
        #[cfg(test)]
        pub(crate) fn measure_with_fit(
            &mut self,
            text: &str,
            shape: TestShape,
            fit: LineFit,
            unbounded_key: TextShapeKey,
        ) -> TestMeasure {
            let request = shape.request(text, fit);
            debug_assert_eq!(request.key.unbounded_version(), unbounded_key);
            self.measure_with_fit_key(request)
        }

        /// Number of shaped buffers currently cached. Reach-in for the
        /// in-tree eviction tests.
        pub(crate) fn cache_len(&self) -> usize {
            self.cache.len()
        }

        /// Outstanding expiry tickets. The number that says whether
        /// [`CosmicMeasure::supersede`] is holding up its end of the
        /// wheel's protocol: a demote files a ticket that supplants the
        /// outstanding one, and if the supplanted ticket re-files itself
        /// this grows by one per demote for as long as the entry lives.
        #[cfg(test)]
        pub(crate) fn pending_tickets(&self) -> usize {
            self.expiry.pending()
        }

        /// Advance the shared clock by one frame and sweep — what
        /// `TextShaper::tick_frame` does for a measurer reached through
        /// a `TextShaper`. The retention tests drive `CosmicMeasure`
        /// directly, with no shaper to hold the clock, so they own the
        /// tick; production never increments here.
        #[cfg(test)]
        pub(crate) fn tick_frame(&mut self) {
            self.end_frame(self.frame + 1);
        }

        /// Drop every shaped buffer now, recycling each one, without
        /// waiting out a retention window. Lets tests that exercise the
        /// *restore* path (which any eviction can trigger) set up a
        /// guaranteed-cold cache in one call, instead of encoding this
        /// cache's retention policy into tests that aren't about it.
        #[cfg(test)]
        pub(crate) fn drop_all_buffers(&mut self) {
            let cache = &mut self.cache;
            let recycle_pool = &mut self.recycle_pool;
            for (_, entry) in cache.drain() {
                recycle_buffer(recycle_pool, entry.buffer);
            }
            self.expiry.clear();
        }

        #[cfg(test)]
        pub(crate) fn recycle_pool_stats(&self) -> RecyclePoolStats {
            RecyclePoolStats {
                len: self.recycle_pool.len(),
                capacity: self.recycle_pool.capacity(),
                limit: RECYCLE_POOL_CAP,
            }
        }

        /// Family name of the font cosmic-text actually shaped `text`
        /// with for `family`. Proves [`attrs_for`] maps each
        /// [`FontFamily`] to the intended physical face — a measured-
        /// width comparison can't, since two different faces can share
        /// an advance.
        #[cfg(test)]
        pub(crate) fn resolved_family(&mut self, text: &str, family: FontFamily) -> Option<String> {
            let mut buf = Buffer::new(&mut self.font_system, Metrics::new(16.0, 19.2));
            buf.set_text(
                text,
                &attrs_for(family, FontWeight::Regular),
                Shaping::Advanced,
                None,
            );
            buf.shape_until_scroll(&mut self.font_system, false);
            let id = buf.layout_runs().next()?.glyphs.first()?.font_id;
            self.font_system
                .db()
                .face(id)
                .map(|f| f.families[0].0.clone())
        }
    }
}
