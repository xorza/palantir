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

use crate::text::cosmic::geometry::{intrinsic_min_width, shaped_geometry};
use crate::text::cosmic::retention::CacheEntry;
use crate::text::cosmic::truncate::EllipsisMemo;

pub(super) mod counters;
pub(super) mod geometry;
pub(super) mod glyphs;
pub(super) mod retention;
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
#[cfg(any(test, feature = "internals"))]
mod internals {
    #![allow(dead_code)]
    use super::*;
    use crate::text::request::internals::TestShape;
    use crate::text::root::internals::TestMeasure;

    #[derive(Debug, PartialEq, Eq)]
    pub(crate) struct RecyclePoolStats {
        pub(crate) len: usize,
        pub(crate) capacity: usize,
        pub(crate) limit: usize,
    }

    impl CosmicMeasure {
        pub(crate) fn measure(&mut self, text: &str, shape: TestShape) -> TestMeasure {
            self.measure_with_fit_key(shape.request(text, LineFit::Wrap))
        }

        /// Shape `request` and pair the result with the key it shaped
        /// under — invalid for empty text, which mints no buffer.
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
        pub(crate) fn pending_tickets(&self) -> usize {
            self.expiry.pending()
        }

        /// Advance the shared clock by one frame and sweep — what
        /// `TextShaper::tick_frame` does for a measurer reached through
        /// a `TextShaper`. The retention tests drive `CosmicMeasure`
        /// directly, with no shaper to hold the clock, so they own the
        /// tick; production never increments here.
        pub(crate) fn tick_frame(&mut self) {
            self.end_frame(self.frame + 1);
        }

        /// Drop every shaped buffer now, recycling each one, without
        /// waiting out a retention window. Lets tests that exercise the
        /// *restore* path (which any eviction can trigger) set up a
        /// guaranteed-cold cache in one call, instead of encoding this
        /// cache's retention policy into tests that aren't about it.
        pub(crate) fn drop_all_buffers(&mut self) {
            let cache = &mut self.cache;
            let recycle_pool = &mut self.recycle_pool;
            for (_, entry) in cache.drain() {
                recycle_buffer(recycle_pool, entry.buffer);
            }
            self.expiry.clear();
        }

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
