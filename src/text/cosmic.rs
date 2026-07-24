//! Real text shaping via [`cosmic_text`]. Caches one shaped `Buffer`
//! per [`TextShapeKey`] — every input that affects shaping (text hash,
//! font size, wrap width, line height, family, weight, halign, fit) —
//! so steady-state measurement is `HashMap` lookup only: no reshape,
//! no allocation. The cache is bounded:
//! [`CosmicMeasure::end_frame_evict`] drops the least-recently-used
//! buffers each frame. Missing buffers are reconstructible from the
//! retained text source at the backend boundary, so a continuous resize
//! drag — every width unique, a fresh entry per run per frame — stays
//! bounded without explicit cache ownership. Evicted buffers feed a
//! bounded recycle pool so later misses retain Cosmic Text's internal
//! line, shaping, and layout allocations.
//!
//! The render side never sees cosmic types: `TextShaper::render_session`
//! lends a `text::render::TextRenderSession` whose
//! [`CosmicMeasure::extract_glyphs`] / [`CosmicMeasure::rasterize_glyph`]
//! translate shaped buffers into aperture-native placements and bitmaps;
//! `text/mod.rs` documents why there's no `TextMeasure` trait.
//!
//! Hash collisions are theoretically possible (we key on a 64-bit hash of the
//! text rather than storing the full string), but at typical UI scales the
//! cost of resolving them — verifying with the cached buffer's source string
//! on every hit — outweighs the cost of accepting the negligible risk.

use crate::layout::types::align::HAlign;
use crate::primitives::num::F32Ext;
use crate::primitives::size::Size;
use crate::text::key::TextShapeKey;
use crate::text::render::{GlyphImage, GlyphImageKind, GlyphPlacement, PlacedGlyph, RunPlacement};
use crate::text::wrap::LineFit;
use crate::text::{FontFamily, FontWeight, TextMeasurement, TextShapeRequest};
use cosmic_text::{
    Align as CosmicAlign, Attrs, Buffer, CacheKey, CacheKeyFlags, Family, FontSystem, Metrics,
    Shaping, SubpixelBin, SwashCache, SwashContent, Weight, fontdb,
};
use glam::Vec2;
use rustc_hash::FxHashMap;
use std::sync::Arc;

/// Bundled fonts shipped with the crate. Inter is the default UI /
/// proportional body font; JetBrains Mono is the monospace. Both ship as
/// a single variable-weight (`wght`) face, so Regular and Bold come from
/// one file each. Both are OFL 1.1. Weight is selected per-run via
/// [`FontWeight`] on the [`crate::TextStyle`], resolved in [`attrs_for`].
const INTER: &[u8] = include_bytes!("../../assets/fonts/Inter-VariableFont_opsz,wght.ttf");
const JBMONO: &[u8] = include_bytes!("../../assets/fonts/JetBrainsMono[wght].ttf");

/// Cap on [`CosmicMeasure::ellipsis_cache`] entries. The cache keys on
/// `(quantized size, family, weight)` — a handful in normal use, but
/// unbounded under a continuous font-size zoom. Cleared wholesale past
/// this; a miss is one cheap "…" shape, so the occasional reset is
/// negligible.
pub(crate) const ELLIPSIS_CACHE_CAP: usize = 128;
const RECYCLE_POOL_CAP: usize = 128;

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

/// Map an Aperture [`HAlign`] to cosmic-text's per-line align.
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
    measured: Size,
    /// Width of the widest unbreakable run, in logical px. Computed only
    /// for unbounded entries; width-bounded entries store `0.0` — bounded
    /// consumers derive floors from the unbounded root, so the per-cluster
    /// word scan is skipped for them.
    intrinsic_min: f32,
    /// `true` when the shaped buffer laid out as one visual line.
    single_line: bool,
    /// Monotonic access generation at the last measure or encode-time
    /// touch. The LRU recency key for [`CosmicMeasure::end_frame_evict`].
    last_used: u64,
}

/// Real-shaping text measurer. Owns a [`FontSystem`] populated by
/// [`CosmicMeasure::with_bundled_fonts`] (Inter + JetBrains Mono) and
/// a cache of shaped `Buffer`s keyed on the inputs that affect shaping.
/// Per-call font family + weight selection comes from [`FontFamily`] /
/// [`FontWeight`] on each measurement; internal named lookups resolve against
/// the bundled set.
pub(crate) struct CosmicMeasure {
    font_system: FontSystem,
    /// Swash rasterization context for [`Self::rasterize_glyph`]. Used
    /// uncached — the renderer's glyph atlas is the real bitmap cache.
    swash_cache: SwashCache,
    cache: FxHashMap<TextShapeKey, CacheEntry>,
    /// Monotonic cache-access counter. Unique recency values let eviction
    /// retain exactly the configured number of most-recent entries.
    use_gen: u64,
    /// Reusable scratch holding every entry's `last_used` during
    /// [`Self::end_frame_evict`], retained so eviction allocates nothing.
    evict_scratch: Vec<u64>,
    /// LIFO pool fed by LRU eviction. `Buffer::set_text` reclaims its
    /// line, shaping, and layout allocations when the buffer is reset.
    recycle_pool: Vec<Buffer>,
    /// Trailing advance of "…" per `(quantized font size, family, weight)`.
    /// The ellipsis width is constant for a given size + face, so this turns
    /// the per-truncation ellipsis reshape into a map lookup (one shape
    /// per distinct size+family+weight, ever).
    ellipsis_cache: FxHashMap<(u32, u8, u8), f32>,
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
    pub(crate) fn with_bundled_fonts() -> Self {
        let sources = [INTER, JBMONO]
            .into_iter()
            .map(|b| fontdb::Source::Binary(Arc::new(b)));
        let font_system = FontSystem::new_with_fonts(sources);
        Self {
            font_system,
            swash_cache: SwashCache::new(),
            cache: FxHashMap::default(),
            use_gen: 0,
            evict_scratch: Vec::new(),
            recycle_pool: Vec::with_capacity(RECYCLE_POOL_CAP),
            ellipsis_cache: FxHashMap::default(),
            truncate_scratch: String::new(),
            break_scratch: Vec::new(),
        }
    }

    /// Look up the shaped buffer for `key`. Returns `None` for keys that
    /// were never measured this `CosmicMeasure` instance — including
    /// [`TextShapeKey::INVALID`].
    pub(crate) fn buffer_for(&self, key: TextShapeKey) -> Option<&Buffer> {
        if key.is_invalid() {
            return None;
        }
        self.cache.get(&key).map(|e| &e.buffer)
    }

    /// Resolve `request` to aperture-native glyph placements for the
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
}

/// Opaque per-glyph rasterization identity: cosmic's `CacheKey` (font,
/// glyph id, scaled size, subpixel bins, flags) behind a newtype so the
/// renderer's atlas can key on it without seeing cosmic types.
/// Constructed and consumed only in this module.
#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub(crate) struct GlyphRasterKey(CacheKey);

/// Split a physical-px origin into its integer part plus cosmic's
/// packed 4-bin subpixel remainder — the exact binning
/// `LayoutGlyph::physical` folds into each glyph's raster key, so the
/// renderer's encoded-run identity can't drift from cosmic's.
pub(crate) fn subpixel_origin(origin: Vec2) -> SubpixelOrigin {
    let (x, x_bin) = SubpixelBin::new(origin.x);
    let (y, y_bin) = SubpixelBin::new(origin.y);
    SubpixelOrigin {
        x,
        y,
        bins: ((x_bin as u8) << 2) | (y_bin as u8),
    }
}

/// [`subpixel_origin`]'s named result.
#[derive(Clone, Copy, Debug)]
pub(crate) struct SubpixelOrigin {
    pub(crate) x: i32,
    pub(crate) y: i32,
    /// Bits 0-1: `y_bin`; bits 2-3: `x_bin` (cosmic's four subpixel
    /// bins, 2 bits each).
    pub(crate) bins: u8,
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
            .field("use_gen", &self.use_gen)
            .finish_non_exhaustive()
    }
}

impl CosmicMeasure {
    #[profiling::function]
    pub(crate) fn shape(&mut self, request: TextShapeRequest<'_>) -> TextMeasurement {
        match (request.key.fit(), request.key.max_width_px()) {
            (LineFit::Clip | LineFit::Ellipsis, Some(_)) => self.measure_truncated(request),
            _ => self.measure_wrapped(request),
        }
    }

    fn measure_wrapped(&mut self, request: TextShapeRequest<'_>) -> TextMeasurement {
        if request.text.is_empty() {
            return TextMeasurement::ZERO;
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
        let last_used = self.next_use_gen();
        self.cache.insert(
            key,
            CacheEntry {
                buffer,
                measured: extent.size,
                intrinsic_min: extent.intrinsic_min,
                single_line: extent.single_line,
                last_used,
            },
        );
        TextMeasurement {
            size: extent.size,
            key,
            intrinsic_min: extent.intrinsic_min,
            single_line: extent.single_line,
        }
    }

    /// Shape `text` as a single line truncated to fit `w`. Truncation is
    /// char-precise: the cached unbounded shape gives per-glyph advances, we
    /// cut at the last glyph whose trailing edge fits, then shape the
    /// (possibly truncated) prefix on one **natural** line — unbounded, no
    /// per-line align. The committed width only decides the cut; the encoder
    /// positions/aligns the single line, so the measured extent is the glyph
    /// width, not `w` (binding to `w` + center align would inflate a
    /// fits-anyway label to ~half the box). `LineFit::Ellipsis` reserves room
    /// for and appends a trailing `…`; `LineFit::Clip` cuts flush to `w`
    /// with no marker. The buffer caches under a fit-discriminated key (so it
    /// can't collide with the wrapped buffer — or the other truncation mode —
    /// at the same width). `intrinsic_min` is 0 — a truncated run can shrink
    /// to nothing.
    fn measure_truncated(&mut self, request: TextShapeRequest<'_>) -> TextMeasurement {
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
            return TextMeasurement::ZERO;
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
        // clip cuts flush to the full available width. Resolved (from
        // the memoized cache) before borrowing the probe, since the
        // rare miss shapes "…" through `&mut self`.
        let mut append_ellipsis = false;
        let avail = if matches!(fit, LineFit::Ellipsis) {
            let ellipsis_w = self.ellipsis_advance(key.size_q, metrics, family, weight);
            append_ellipsis = ellipsis_w <= width;
            (width - ellipsis_w).max(0.0)
        } else {
            width
        };
        let probe = &self
            .cache
            .get(&unbounded.key)
            .expect("truncation requires the cached unbounded shape")
            .buffer;
        let line_w = first_line_right(probe);
        let multiline = probe.layout_runs().nth(1).is_some();

        let truncated = if line_w <= width && !multiline {
            false
        } else {
            let mut cut = 0usize;
            if let Some(run) = probe.layout_runs().next() {
                for g in run.glyphs {
                    if g.x + g.w > avail {
                        break;
                    }
                    cut = g.end;
                }
            }
            self.truncate_scratch.clear();
            self.truncate_scratch
                .push_str(request.text[..cut].trim_end());
            if append_ellipsis {
                self.truncate_scratch.push('…');
            }
            true
        };

        // Shape unbounded on one line: the cut already fit it to `w`, and the
        // encoder owns single-line placement. Binding to `Some(w)` + align
        // would measure the aligned glyph position, inflating a fits-anyway
        // label toward the box width.
        let mut buffer = self.acquire_buffer(metrics, None);
        let shaped_text = if truncated {
            self.truncate_scratch.as_str()
        } else {
            request.text
        };
        buffer.set_text(shaped_text, &attrs, Shaping::Advanced, None);
        buffer.shape_until_scroll(&mut self.font_system, false);

        let measured = shaped_extent(&buffer, None).size;
        let last_used = self.next_use_gen();
        // Truncated runs are one natural line by construction: the cut
        // prefix comes from the unbounded probe's first layout run.
        self.cache.insert(
            key,
            CacheEntry {
                buffer,
                measured,
                intrinsic_min: 0.0,
                single_line: true,
                last_used,
            },
        );
        TextMeasurement {
            size: measured,
            key,
            intrinsic_min: 0.0,
            single_line: true,
        }
    }

    /// Trailing advance of "…" at `metrics`/`family`/`weight`, memoized per
    /// `(quantized size, family, weight)`. The width is constant for a given
    /// size + face, so this is a map lookup after the first shape. The
    /// rare miss shapes into a temporary buffer so the cached unbounded
    /// probe remains immutable.
    fn ellipsis_advance(
        &mut self,
        size_q: u32,
        metrics: Metrics,
        family: FontFamily,
        weight: FontWeight,
    ) -> f32 {
        let key = (size_q, family as u8, weight as u8);
        if let Some(&w) = self.ellipsis_cache.get(&key) {
            return w;
        }
        let mut buffer = self.acquire_buffer(metrics, None);
        buffer.set_text("…", &attrs_for(family, weight), Shaping::Advanced, None);
        buffer.shape_until_scroll(&mut self.font_system, false);
        let w = first_line_right(&buffer);
        recycle_buffer(&mut self.recycle_pool, buffer);
        // Bounded: the key space is (discrete font sizes × families × weights)
        // and normally tiny, but a continuous font-size zoom over ellipsized
        // text mints a new quantized size each frame. Entries are trivially
        // recomputable (one "…" shape), so clear wholesale on overflow
        // rather than track recency.
        if self.ellipsis_cache.len() >= ELLIPSIS_CACHE_CAP {
            self.ellipsis_cache.clear();
        }
        self.ellipsis_cache.insert(key, w);
        w
    }

    /// Restore a missing shaped buffer from the retained source text and
    /// the canonical parameters encoded by `key`. Truncated runs restore
    /// their unbounded probe first; callers never manage that dependency.
    pub(crate) fn ensure_buffer(&mut self, request: TextShapeRequest<'_>) {
        if request.key.is_invalid() || self.cache_hit(request.key).is_some() {
            return;
        }
        let result = self.shape(request);
        assert_eq!(
            result.key, request.key,
            "restored text buffer did not reproduce its TextShapeKey",
        );
    }

    fn next_use_gen(&mut self) -> u64 {
        let next = self.use_gen;
        self.use_gen = self
            .use_gen
            .checked_add(1)
            .expect("text cache LRU generation overflowed");
        next
    }

    /// A cached entry's `TextMeasurement` for `key`, or `None` on a miss.
    /// Refreshes `last_used` for both layout-time hits and encoder ensures.
    fn cache_hit(&mut self, key: TextShapeKey) -> Option<TextMeasurement> {
        let now = self.next_use_gen();
        self.cache.get_mut(&key).map(|entry| {
            entry.last_used = now;
            TextMeasurement {
                size: entry.measured,
                key,
                intrinsic_min: entry.intrinsic_min,
                single_line: entry.single_line,
            }
        })
    }

    fn acquire_buffer(&mut self, metrics: Metrics, width: Option<f32>) -> Buffer {
        let mut buffer = match self.recycle_pool.pop() {
            Some(buffer) => buffer,
            None => Buffer::new(&mut self.font_system, metrics),
        };
        buffer.set_metrics_and_size(metrics, width, None);
        buffer
    }

    /// Retain the `max_keep` most-recently-used buffers. Every entry is
    /// reconstructible at encode, so no owner or layout can pin a key.
    pub(crate) fn end_frame_evict(&mut self, max_keep: usize) {
        if self.cache.len() <= max_keep {
            return;
        }
        if max_keep == 0 {
            let cache = &mut self.cache;
            let recycle_pool = &mut self.recycle_pool;
            for (_, entry) in cache.drain() {
                recycle_buffer(recycle_pool, entry.buffer);
            }
            return;
        }
        self.evict_scratch.clear();
        self.evict_scratch
            .extend(self.cache.values().map(|entry| entry.last_used));
        let cut = self.evict_scratch.len() - max_keep;
        let (_, &mut cutoff, _) = self.evict_scratch.select_nth_unstable(cut);
        let cache = &mut self.cache;
        let recycle_pool = &mut self.recycle_pool;
        for (_, entry) in cache.extract_if(|_, entry| entry.last_used < cutoff) {
            recycle_buffer(recycle_pool, entry.buffer);
        }
        debug_assert_eq!(self.cache.len(), max_keep);
    }
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

#[cfg(test)]
mod test_support {
    use super::*;

    impl GlyphRasterKey {
        /// Distinct dummy keys for the renderer's atlas tests — the
        /// only way to mint one outside a real glyph walk.
        pub(crate) fn for_test(glyph_id: u16) -> Self {
            Self(CacheKey {
                font_id: fontdb::ID::dummy(),
                glyph_id,
                font_size_bits: 14.0_f32.to_bits(),
                x_bin: SubpixelBin::Zero,
                y_bin: SubpixelBin::Zero,
                font_weight: Weight::NORMAL,
                flags: CacheKeyFlags::empty(),
            })
        }
    }

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

        /// Number of memoized ellipsis advances. Reach-in for the
        /// ellipsis-cache-bound test.
        pub(crate) fn ellipsis_cache_len(&self) -> usize {
            self.ellipsis_cache.len()
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
