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
//! The render side reaches the cached `Buffer`s and the `FontSystem`
//! through [`CosmicMeasure::split_for_render`] (via
//! `TextShaper::with_render_buffers`) — a disjoint borrow, not a trait
//! object; `text/mod.rs` documents why there's no `TextMeasure` trait.
//!
//! Hash collisions are theoretically possible (we key on a 64-bit hash of the
//! text rather than storing the full string), but at typical UI scales the
//! cost of resolving them — verifying with the cached buffer's source string
//! on every hit — outweighs the cost of accepting the negligible risk.

use crate::layout::types::align::HAlign;
use crate::primitives::size::Size;
use crate::text::{
    FontFamily, FontWeight, LineFit, TextMeasurement, TextShapeKey, TextShapeRequest,
};
use cosmic_text::{
    Align as CosmicAlign, Attrs, Buffer, CacheKeyFlags, Family, FontSystem, Metrics, Shaping,
    Weight, fontdb,
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
    /// Width of the widest unbreakable run, in logical px. Computed once on
    /// insert from the unbounded shaping result and reused for every later
    /// `measure` call that hits this entry.
    intrinsic_min: f32,
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
#[derive(Debug)]
pub struct CosmicMeasure {
    font_system: FontSystem,
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
    pub fn with_bundled_fonts() -> Self {
        let sources = [INTER, JBMONO]
            .into_iter()
            .map(|b| fontdb::Source::Binary(Arc::new(b)));
        let font_system = FontSystem::new_with_fonts(sources);
        Self {
            font_system,
            cache: FxHashMap::default(),
            use_gen: 0,
            evict_scratch: Vec::new(),
            recycle_pool: Vec::with_capacity(RECYCLE_POOL_CAP),
            ellipsis_cache: FxHashMap::default(),
            truncate_scratch: String::new(),
        }
    }

    /// Look up the shaped buffer for `key`. Returns `None` for keys that
    /// were never measured this `CosmicMeasure` instance — including
    /// [`TextShapeKey::INVALID`].
    pub(crate) fn buffer_for(&self, key: TextShapeKey) -> Option<&Buffer> {
        BufferLookup { cache: &self.cache }.get(key)
    }

    /// Split borrow: `font_system` + `lookup`. Glyphon's `prepare` needs
    /// `&mut FontSystem` while we iterate `RenderBuffer.text_runs` and look
    /// up buffers — borrowck won't let us hand out a `&mut FontSystem` and
    /// call `buffer_for` simultaneously through `&mut self`. This method
    /// hands out the disjoint pieces.
    pub(crate) fn split_for_render(&mut self) -> RenderSplit<'_> {
        RenderSplit {
            font_system: &mut self.font_system,
            lookup: BufferLookup { cache: &self.cache },
        }
    }
}

/// Disjoint borrow handed out by [`CosmicMeasure::split_for_render`].
pub(crate) struct RenderSplit<'a> {
    pub font_system: &'a mut FontSystem,
    pub lookup: BufferLookup<'a>,
}

/// Read-only view into the buffer cache. Constructed by
/// [`CosmicMeasure::split_for_render`]; held alongside a `&mut FontSystem`.
pub(crate) struct BufferLookup<'a> {
    cache: &'a FxHashMap<TextShapeKey, CacheEntry>,
}

impl<'a> BufferLookup<'a> {
    pub(crate) fn get(&self, key: TextShapeKey) -> Option<&'a Buffer> {
        if key.is_invalid() {
            return None;
        }
        self.cache.get(&key).map(|e| &e.buffer)
    }
}

impl Default for CosmicMeasure {
    fn default() -> Self {
        Self::with_bundled_fonts()
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

        let extent = shaped_extent(&buffer);
        let last_used = self.next_use_gen();
        self.cache.insert(
            key,
            CacheEntry {
                buffer,
                measured: extent.size,
                intrinsic_min: extent.intrinsic_min,
                last_used,
            },
        );
        TextMeasurement {
            size: extent.size,
            key,
            intrinsic_min: extent.intrinsic_min,
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
            // Reserve the ellipsis width only when we'll append one; a plain
            // clip cuts flush to the full available width.
            let mut append_ellipsis = false;
            let avail = if matches!(fit, LineFit::Ellipsis) {
                let ellipsis_w = self.ellipsis_advance(key.size_q, metrics, family, weight);
                append_ellipsis = ellipsis_w <= width;
                (width - ellipsis_w).max(0.0)
            } else {
                width
            };
            let mut cut = 0usize;
            let probe = &self
                .cache
                .get(&unbounded.key)
                .expect("unbounded shape disappeared during truncation")
                .buffer;
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

        let measured = shaped_extent(&buffer).size;
        let last_used = self.next_use_gen();
        self.cache.insert(
            key,
            CacheEntry {
                buffer,
                measured,
                intrinsic_min: 0.0,
                last_used,
            },
        );
        TextMeasurement {
            size: measured,
            key,
            intrinsic_min: 0.0,
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

/// Trailing edge (`x + w` of the last glyph) of a shaped buffer's first
/// layout run, or `0.0` when empty — the rendered width of one line. The
/// per-run analogue inside [`shaped_extent`] takes the max across runs.
fn first_line_right(buffer: &Buffer) -> f32 {
    buffer
        .layout_runs()
        .next()
        .and_then(|r| r.glyphs.last().map(|g| g.x + g.w))
        .unwrap_or(0.0)
}

/// Measured extent of a shaped `buffer`: bounding size (ceil'd) plus the
/// widest unbreakable run (longest word), the floor the wrap path uses
/// when a parent commits a narrower width.
struct ShapedExtent {
    size: Size,
    intrinsic_min: f32,
}

fn shaped_extent(buffer: &Buffer) -> ShapedExtent {
    let mut max_w = 0.0_f32;
    let mut total_h = 0.0_f32;
    let mut intrinsic_min = 0.0_f32;
    let mut current_word_w = 0.0_f32;
    for run in buffer.layout_runs() {
        // `line_w` is content width before per-line alignment; when
        // align shifts glyphs right, the glyph cluster's physical x
        // extends past `line_w`. Take the last glyph's trailing edge so
        // the measured bbox encloses every rendered pixel — otherwise
        // the text backend clips right-aligned glyphs against an
        // undersized `TextBounds`.
        let line_right = run.glyphs.last().map(|g| g.x + g.w).unwrap_or(run.line_w);
        max_w = max_w.max(line_right);
        total_h = total_h.max(run.line_top + run.line_height);
        for g in run.glyphs {
            let cluster = &run.text[g.start..g.end];
            let is_break = cluster.chars().all(|c| c.is_whitespace());
            if is_break {
                intrinsic_min = intrinsic_min.max(current_word_w);
                current_word_w = 0.0;
            } else {
                current_word_w += g.w;
            }
        }
        // Hard line break (\n) terminates a run — also closes any
        // in-progress word.
        intrinsic_min = intrinsic_min.max(current_word_w);
        current_word_w = 0.0;
    }
    ShapedExtent {
        size: Size::new(max_w.ceil(), total_h.ceil()),
        intrinsic_min,
    }
}

#[cfg(test)]
mod test_support {
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
