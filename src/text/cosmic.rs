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
//! line, shaping, and layout allocations.
//!
//! The render side never sees cosmic types: `TextShaper::render_session`
//! lends a `RefMut<CosmicMeasure>` whose
//! [`CosmicMeasure::extract_glyphs`] / [`CosmicMeasure::rasterize_glyph`]
//! translate shaped buffers into palantir-native placements and bitmaps;
//! `text/mod.rs` documents why there's no `TextMeasure` trait.
//!
//! Hash collisions are theoretically possible (we key on a 64-bit hash of the
//! text rather than storing the full string), but at typical UI scales the
//! cost of resolving them — verifying with the cached buffer's source string
//! on every hit — outweighs the cost of accepting the negligible risk.

use crate::layout::types::align::HAlign;
use crate::primitives::num::F32Ext;
use crate::primitives::size::Size;
use crate::text::cache_probe::CacheProbe;
use crate::text::key::TextShapeKey;
use crate::text::render::{
    GlyphImage, GlyphImageKind, GlyphPlacement, GlyphRasterKey, PlacedGlyph, RunPlacement,
};
use crate::text::wrap::LineFit;
use crate::text::{FontFamily, FontWeight, TextRoot, TextShapeRequest};
use cosmic_text::{
    Align as CosmicAlign, Attrs, Buffer, CacheKeyFlags, Family, FontSystem, Metrics, Shaping,
    SwashCache, SwashContent, Weight, fontdb,
};
use rustc_hash::FxHashMap;
use std::sync::Arc;

/// Bundled fonts shipped with the crate. Inter is the default UI /
/// proportional body font; JetBrains Mono is the monospace. Both ship as
/// a single variable-weight (`wght`) face, so Regular and Bold come from
/// one file each. Both are OFL 1.1. Weight is selected per-run via
/// [`FontWeight`] on the [`crate::TextStyle`], resolved in [`attrs_for`].
const INTER: &[u8] = include_bytes!("../../assets/fonts/Inter-VariableFont_opsz,wght.ttf");
const JBMONO: &[u8] = include_bytes!("../../assets/fonts/JetBrainsMono[wght].ttf");

const RECYCLE_POOL_CAP: usize = 128;

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
pub(super) const PROBATION_KEEP_FRAMES: u64 = 4;

/// Frames a *protected* entry — one that has been looked up at least once
/// since insertion — survives untouched.
///
/// Matches `ENCODED_CACHE_KEEP_FRAMES` in the backend text encoder,
/// deliberately: that cache is what generates the render-side lookups
/// this one exists to answer, so a buffer should outlive the encoded
/// entry that would come asking for it.
pub(super) const PROTECTED_KEEP_FRAMES: u64 = 120;

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

#[derive(Debug)]
struct CacheEntry {
    /// Shaped buffer. Looked up by [`TextShapeKey`] at render time so the
    /// text backend can build a `TextArea` without reshaping.
    buffer: Buffer,
    /// What this buffer measured to. Bounded entries carry a zero floor
    /// and a single-line flag describing the resolve, both inert — only
    /// the unbounded root's copy is ever read back.
    root: TextRoot,
    /// Last frame on which this entry is kept; [`CosmicMeasure::end_frame`]
    /// drops it once the clock passes this. Insertion sets it one
    /// probation window out and every lookup pushes it a protected window
    /// out, so the two-tier policy needs no separate "has been reused"
    /// flag — the deadline *is* the tier.
    keep_until: u64,
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
    /// Frames elapsed, bumped by [`Self::end_frame`]. Stamped onto every
    /// entry it touches, and the reference point both retention windows
    /// measure back from.
    frame: u64,
    /// Earliest `keep_until` in the cache, or `u64::MAX` when it is
    /// empty. Lets [`Self::end_frame`] skip the sweep in O(1) while no
    /// entry can have expired.
    ///
    /// Only ever conservative: a lookup extends an entry's deadline
    /// without advancing this, so a stale value costs one sweep that
    /// drops nothing and then recomputes itself exactly. It is never too
    /// late, which is the direction that would let an expired entry live.
    next_expiry: u64,
    /// LIFO pool fed by LRU eviction. `Buffer::set_text` reclaims its
    /// line, shaping, and layout allocations when the buffer is reset.
    recycle_pool: Vec<Buffer>,
    /// Trailing advance of "…" for the most recent face.
    ///
    /// One slot, not a map: a frame draws its ellipsized labels in one or
    /// two text styles, so this hits nearly always, and a miss is a single
    /// glyph through a recycled buffer. Nothing to bound, evict, or clear —
    /// the slot is simply overwritten.
    ellipsis: Option<EllipsisMemo>,
    /// Retained scratch for the truncated string
    /// [`Self::measure_truncated`] builds on a miss (cut prefix +
    /// optional `…`). Misses are the hot case — a continuous width drag
    /// mints a fresh quantized target per label per frame — so building
    /// into a retained buffer keeps that path free of `String` allocs,
    /// while the unbounded probe itself comes from `cache`.
    truncate_scratch: String,
    /// Retained scratch for [`collect_break_offsets`], so the unbounded
    /// shape's segment scan allocates nothing per miss.
    break_scratch: Vec<u32>,
    /// Retained scratch holding the truncation probe's glyph indices in
    /// logical order — visual order is what the shaped run gives us, and
    /// truncation needs the logical prefix.
    logical_order: Vec<u32>,
    /// Shape / hit / supersede / expire tallies. Zero-sized outside
    /// tests.
    pub(super) probe: CacheProbe,
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
            next_expiry: u64::MAX,
            recycle_pool: Vec::with_capacity(RECYCLE_POOL_CAP),
            ellipsis: None,
            truncate_scratch: String::new(),
            break_scratch: Vec::new(),
            logical_order: Vec::new(),
            probe: CacheProbe::default(),
        }
    }

    /// Look up the shaped buffer for `key`. Returns `None` for keys that
    /// were never measured this `CosmicMeasure` instance — including
    /// [`TextShapeKey::INVALID`].
    pub(super) fn buffer_for(&self, key: TextShapeKey) -> Option<&Buffer> {
        if key.is_invalid() {
            return None;
        }
        self.cache.get(&key).map(|e| &e.buffer)
    }

    /// Resolve `request` to palantir-native glyph placements for the
    /// renderer. Restores the shaped buffer if evicted (truncated runs
    /// restore their unbounded probe internally), walks its layout runs,
    /// y-culls whole lines against `placement.bounds`, and rewrites
    /// `out` with one [`PlacedGlyph`] per surviving glyph. Returns
    /// whether any line was culled — such partial extractions must not
    /// become renderer cache templates (its encoded key carries no
    /// bounds).
    pub(super) fn extract_glyphs(
        &mut self,
        request: TextShapeRequest<'_>,
        placement: RunPlacement,
        out: &mut Vec<PlacedGlyph>,
    ) -> bool {
        debug_assert!(!request.key.is_invalid());
        self.ensure_buffer(request);
        let buffer = self
            .buffer_for(request.key)
            .expect("ensure_buffer must restore the requested render buffer");

        out.clear();
        let RunPlacement {
            origin,
            scale,
            bounds,
        } = placement;
        let bounds_top = bounds.y as f32;
        let bounds_bot = (bounds.y + bounds.h) as f32;
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
                let physical = glyph.physical((origin.x, origin.y), scale);
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
    pub(super) fn rasterize_glyph(&mut self, key: GlyphRasterKey) -> Option<GlyphImage> {
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

impl CosmicMeasure {
    #[profiling::function]
    pub(super) fn shape(&mut self, request: TextShapeRequest<'_>) -> TextRoot {
        match (request.key.fit(), request.key.max_width_px()) {
            (LineFit::Clip | LineFit::Ellipsis, Some(_)) => self.measure_truncated(request),
            _ => self.measure_wrapped(request),
        }
    }

    fn measure_wrapped(&mut self, request: TextShapeRequest<'_>) -> TextRoot {
        if request.text.is_empty() {
            return TextRoot::ZERO;
        }
        let key = request.key;
        if let Some(hit) = self.cache_hit(key) {
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

        let extent = shaped_extent(
            &buffer,
            key.max_width_px()
                .is_none()
                .then_some(&mut self.break_scratch),
        );
        let root = TextRoot {
            size: extent.size,
            intrinsic_min: extent.intrinsic_min,
            single_line: extent.single_line,
        };
        self.insert(key, buffer, root);
        root
    }

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
    fn measure_truncated(&mut self, request: TextShapeRequest<'_>) -> TextRoot {
        let key = request.key;
        let fit = key.fit();
        let width = key
            .max_width_px()
            .expect("measure_truncated requires a finite width");
        debug_assert!(
            matches!(fit, LineFit::Clip | LineFit::Ellipsis),
            "measure_truncated requires Clip or Ellipsis",
        );
        if request.text.is_empty() {
            return TextRoot::ZERO;
        }
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
        let fits_whole = {
            let probe = &self
                .cache
                .get(&probe_key)
                .expect("truncation requires the cached unbounded shape")
                .buffer;
            first_line_right(probe) <= width && probe.layout_runs().nth(1).is_none()
        };

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
            shaped_extent(&buffer, None).size
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
                let cut = {
                    let probe = &self
                        .cache
                        .get(&probe_key)
                        .expect("truncation requires the cached unbounded shape")
                        .buffer;
                    match probe.layout_runs().next() {
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
                    }
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
                let size = shaped_extent(&buffer, None).size;
                if size.w <= width || cut == 0 {
                    break size;
                }
                max_end = cut;
            }
        };

        // Truncated runs are one natural line by construction: the cut
        // prefix comes from the unbounded probe's first layout run, and a
        // truncated run can shrink to nothing, so its floor is zero.
        let root = TextRoot {
            size,
            intrinsic_min: 0.0,
            single_line: true,
        };
        self.insert(key, buffer, root);
        root
    }

    /// Trailing advance of "…" at `metrics`/`family`/`weight`, memoized for
    /// the last face asked about.
    ///
    /// Only the *opening* budget: [`Self::measure_truncated`] verifies the
    /// shaped result against the committed width either way, so a stale or
    /// imprecise reservation costs retries, never correctness. What it buys
    /// is measured — dropping the memo entirely costs ~29% on the
    /// truncation-miss path (`text_shape/ellipsis_width_churn`).
    fn ellipsis_advance(
        &mut self,
        size_q: u32,
        metrics: Metrics,
        family: FontFamily,
        weight: FontWeight,
    ) -> f32 {
        let want = EllipsisMemo {
            size_q,
            family_q: family as u8,
            weight_q: weight as u8,
            advance: 0.0,
        };
        if let Some(memo) = self.ellipsis
            && memo.same_face(&want)
        {
            return memo.advance;
        }
        let mut buffer = self.acquire_buffer(metrics, None);
        buffer.set_text("…", &attrs_for(family, weight), Shaping::Advanced, None);
        buffer.shape_until_scroll(&mut self.font_system, false);
        let advance = first_line_right(&buffer);
        recycle_buffer(&mut self.recycle_pool, buffer);
        self.ellipsis = Some(EllipsisMemo { advance, ..want });
        advance
    }

    /// Restore a missing shaped buffer from the retained source text and
    /// the canonical parameters encoded by `key`. Truncated runs restore
    /// their unbounded probe first; callers never manage that dependency.
    pub(super) fn ensure_buffer(&mut self, request: TextShapeRequest<'_>) {
        if request.key.is_invalid() || self.cache_hit(request.key).is_some() {
            return;
        }
        self.shape(request);
        assert!(
            self.cache.contains_key(&request.key),
            "restored text buffer did not land under its own TextShapeKey",
        );
    }

    /// Store a freshly shaped buffer. Entries start probationary; only a
    /// later lookup promotes them (see [`PROBATION_KEEP_FRAMES`]).
    fn insert(&mut self, key: TextShapeKey, buffer: Buffer, root: TextRoot) {
        // Counted here rather than per `shape_until_scroll` so one
        // cached run is one tally: `measure_truncated`'s back-off can
        // reshape a prefix several times to land inside the committed
        // width, and a workload test cares that the run was shaped, not
        // how many attempts the cut took. The memoized ellipsis probe
        // shapes without inserting and is deliberately not counted.
        self.probe.shape();
        let keep_until = self.frame + PROBATION_KEEP_FRAMES;
        self.next_expiry = self.next_expiry.min(keep_until);
        self.cache.insert(
            key,
            CacheEntry {
                buffer,
                root,
                keep_until,
            },
        );
    }

    /// A cached entry's [`TextRoot`] for `key`, or `None` on a miss.
    /// Layout hits and encoder ensures both land here, and both push the
    /// entry's deadline out to the protected window — being asked for at
    /// all is the evidence that separates reuse from scan traffic, so no
    /// separate promotion step is needed.
    fn cache_hit(&mut self, key: TextShapeKey) -> Option<TextRoot> {
        let keep_until = self.frame + PROTECTED_KEEP_FRAMES;
        let hit = self.cache.get_mut(&key).map(|entry| {
            entry.keep_until = keep_until;
            entry.root
        });
        if hit.is_some() {
            self.probe.hit();
        }
        hit
    }

    /// Demote `key` to the probation window: the reuse slot that owned
    /// it now answers a different key, so nothing can ask for it through
    /// that slot again. See [`PROBATION_KEEP_FRAMES`] for why this is
    /// the signal the two-tier policy runs on.
    ///
    /// Only ever shortens a deadline — a supersede must not extend the
    /// life of an entry already closer to expiry — and pulls
    /// [`Self::next_expiry`] back with it, since the sweep gate would
    /// otherwise sleep straight through the shortened window.
    ///
    /// Silent on a key that isn't resident: the buffer may already have
    /// aged out, and superseding what is gone is a no-op, not an error.
    pub(super) fn supersede(&mut self, key: TextShapeKey) {
        if key.is_invalid() {
            return;
        }
        let keep_until = self.frame + PROBATION_KEEP_FRAMES;
        let Some(entry) = self.cache.get_mut(&key) else {
            return;
        };
        self.probe.supersede();
        // Never *extends* a life: an entry already closer to expiry —
        // one that was inserted and never looked up — keeps its own
        // deadline.
        if entry.keep_until > keep_until {
            entry.keep_until = keep_until;
            self.next_expiry = self.next_expiry.min(keep_until);
        }
    }

    fn acquire_buffer(&mut self, metrics: Metrics, width: Option<f32>) -> Buffer {
        let mut buffer = match self.recycle_pool.pop() {
            Some(buffer) => buffer,
            None => Buffer::new(&mut self.font_system, metrics),
        };
        buffer.set_metrics_and_size(metrics, width, None);
        buffer
    }

    /// Advance the frame clock and drop every buffer whose deadline has
    /// passed.
    ///
    /// Age, not capacity. A count budget cannot express what this cache
    /// needs: set below the live working set it thrashes — UI redraw is a
    /// cyclic access pattern, LRU's worst case, so the overflow misses
    /// every frame forever — and set above it, a resize drag fills it with
    /// widths that can never be hit again. Ageing bounds both without a
    /// number to guess: an app keeps exactly what it keeps touching, and
    /// scan traffic falls out on its own.
    ///
    /// [`Self::next_expiry`] skips the sweep outright while nothing can
    /// have expired, so a steady frame pays one comparison and a churning
    /// one pays in proportion to its churn.
    pub(super) fn end_frame(&mut self) {
        self.frame += 1;
        if self.frame <= self.next_expiry {
            return;
        }
        let frame = self.frame;
        let mut next_expiry = u64::MAX;
        let cache = &mut self.cache;
        let recycle_pool = &mut self.recycle_pool;
        let probe = &mut self.probe;
        for (_, entry) in cache.extract_if(|_, entry| {
            if entry.keep_until < frame {
                return true;
            }
            next_expiry = next_expiry.min(entry.keep_until);
            false
        }) {
            probe.expire();
            recycle_buffer(recycle_pool, entry.buffer);
        }
        self.next_expiry = next_expiry;
    }
}

/// Memoized trailing advance of "…" for one face.
#[derive(Clone, Copy, Debug)]
struct EllipsisMemo {
    size_q: u32,
    family_q: u8,
    weight_q: u8,
    advance: f32,
}

impl EllipsisMemo {
    /// Whether both were shaped from the same face at the same size.
    fn same_face(&self, other: &Self) -> bool {
        self.size_q == other.size_q
            && self.family_q == other.family_q
            && self.weight_q == other.weight_q
    }
}

/// One shaped glyph reduced to what the truncation cut reads: the source
/// bytes it covers and the advance it costs.
#[derive(Clone, Copy, Debug)]
pub(super) struct ClusterGlyph {
    pub(super) start: usize,
    pub(super) end: usize,
    pub(super) advance: f32,
}

/// Longest logical byte prefix of `count` shaped glyphs whose advances sum
/// within `avail` and stay below `max_end`. `order` is retained scratch,
/// refilled with the glyph indices sorted into logical order; `glyph` reads
/// one glyph by its original (visual-order) index.
///
/// The result is always strictly below `max_end`, so passing the previous
/// answer retires at least one more cluster — that is what makes
/// [`CosmicMeasure::measure_truncated`]'s back-off terminate. Pass
/// `usize::MAX` for an unbounded first cut.
///
/// Glyphs arrive in visual order, so a glyph's `x` follows the reading
/// direction rather than the logical prefix — an RTL run's first glyph sits
/// at the *right* edge and its trailing edges descend. Summing advances in
/// logical order instead makes `text[..cut]` the prefix that fits whichever
/// way the run reads.
///
/// One grapheme cluster can shape to several glyphs sharing a byte range
/// (flag and ZWJ emoji, Indic conjuncts), so the prefix only advances past a
/// cluster once every glyph covering it is paid for. Committing mid-cluster
/// would claim bytes whose advance the budget never covered, and the prefix
/// would reshape wider than `avail`.
pub(super) fn fitting_prefix(
    count: usize,
    glyph: impl Fn(usize) -> ClusterGlyph,
    order: &mut Vec<u32>,
    avail: f32,
    max_end: usize,
) -> usize {
    order.clear();
    order.extend(0..count as u32);
    order.sort_unstable_by_key(|&i| glyph(i as usize).start);
    let mut cut = 0usize;
    let mut used = 0.0_f32;
    for (pos, &i) in order.iter().enumerate() {
        let g = glyph(i as usize);
        // Ends are non-decreasing in logical order, so once one reaches the
        // bound no later glyph can be committed either.
        if g.end >= max_end {
            break;
        }
        used += g.advance;
        if used > avail {
            break;
        }
        let cluster_paid = order
            .get(pos + 1)
            .is_none_or(|&next| glyph(next as usize).start >= g.end);
        if cluster_paid {
            cut = g.end;
        }
    }
    cut
}

/// Right edge (widest `x + w` across glyphs — an RTL run's last glyph is
/// its leftmost) of a shaped buffer's first layout run, or `0.0` when
/// empty — the rendered width of one line. The per-run analogue inside
/// [`shaped_extent`] takes the max across runs.
fn first_line_right(buffer: &Buffer) -> f32 {
    buffer
        .layout_runs()
        .next()
        .and_then(|r| r.glyphs.iter().map(|g| g.x + g.w).reduce(f32::max))
        .unwrap_or(0.0)
}

/// Measured extent of a shaped `buffer`: bounding size (ceil'd) plus the
/// widest unbreakable segment, the floor the wrap path uses when a parent
/// commits a narrower width. Passing `breaks` opts into the
/// text-length-proportional segment scan (it doubles as that scan's
/// scratch); bounded shapes pass `None` — their floor comes from the
/// unbounded root — and report `0.0`.
struct ShapedExtent {
    size: Size,
    intrinsic_min: f32,
    single_line: bool,
}

fn shaped_extent(buffer: &Buffer, breaks: Option<&mut Vec<u32>>) -> ShapedExtent {
    let mut max_w = 0.0_f32;
    let mut total_h = 0.0_f32;
    let mut runs = 0usize;
    for run in buffer.layout_runs() {
        runs += 1;
        // `line_w` is content width before per-line alignment; when
        // align shifts glyphs right, the glyph cluster's physical x
        // extends past `line_w`. Take the last glyph's trailing edge so
        // the measured bbox encloses every rendered pixel — otherwise
        // the text backend clips right-aligned glyphs against an
        // undersized `TextBounds`.
        // Max, not the last glyph: an RTL run reads right-to-left, so its
        // last glyph is the *leftmost* one.
        let line_right = run
            .glyphs
            .iter()
            .map(|g| g.x + g.w)
            .reduce(f32::max)
            .unwrap_or(run.line_w);
        max_w = max_w.max(line_right);
        total_h = total_h.max(run.line_top + run.line_height);
    }
    ShapedExtent {
        size: Size::new(max_w.ceil(), total_h.ceil()),
        intrinsic_min: breaks.map_or(0.0, |breaks| intrinsic_min_width(buffer, breaks)),
        single_line: runs <= 1,
    }
}

/// Byte offsets in `text` that start a new unbreakable segment, i.e. the
/// UAX #14 opportunities minus the terminal one at `text.len()`, which
/// ends the text rather than opening a segment.
///
/// Same source cosmic-text splits its shape words on
/// (`cosmic-text/src/shape.rs`), so the wrap floor this feeds cannot
/// claim a segment the shaper would happily break.
fn collect_break_offsets(text: &str, out: &mut Vec<u32>) {
    out.clear();
    out.extend(
        unicode_linebreak::linebreaks(text)
            .map(|(offset, _)| offset)
            .filter(|&offset| offset < text.len())
            .map(|offset| offset as u32),
    );
}

/// Width of the widest segment no line break can split — the min-content
/// width. Trailing whitespace is excluded because UAX #14 places its
/// break opportunity *after* a space, so a space always ends its segment
/// and hangs rather than widening it; interior non-breaking whitespace
/// (U+00A0 and friends) opens no opportunity and so counts in full.
fn intrinsic_min_width(buffer: &Buffer, breaks: &mut Vec<u32>) -> f32 {
    let mut intrinsic_min = 0.0_f32;
    for run in buffer.layout_runs() {
        collect_break_offsets(run.text, breaks);
        let mut segment_w = 0.0_f32;
        let mut trailing_ws_w = 0.0_f32;
        for g in run.glyphs {
            // Glyphs arrive in visual order, but a segment's glyphs stay
            // contiguous within a level run, so entering a new segment
            // closes the previous one whichever way the run reads.
            if breaks.binary_search(&(g.start as u32)).is_ok() {
                intrinsic_min = intrinsic_min.max(segment_w);
                segment_w = 0.0;
                trailing_ws_w = 0.0;
            }
            if run.text[g.start..g.end].chars().all(char::is_whitespace) {
                trailing_ws_w += g.w;
            } else {
                segment_w += trailing_ws_w + g.w;
                trailing_ws_w = 0.0;
            }
        }
        intrinsic_min = intrinsic_min.max(segment_w);
    }
    intrinsic_min
}

// Wider than `cfg(test)`: `drop_all_buffers` is reached from the
// `internals`-gated GPU tests in `renderer::backend::text`, which build
// without `cfg(test)`.
#[cfg(any(test, feature = "internals"))]
mod internals {
    #![allow(dead_code)]
    use super::*;

    #[derive(Debug, PartialEq, Eq)]
    pub(crate) struct RecyclePoolStats {
        pub(crate) len: usize,
        pub(crate) capacity: usize,
        pub(crate) limit: usize,
    }

    impl CosmicMeasure {
        /// Number of shaped buffers currently cached. Reach-in for the
        /// in-tree eviction tests.
        pub(crate) fn cache_len(&self) -> usize {
            self.cache.len()
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
            self.next_expiry = u64::MAX;
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
