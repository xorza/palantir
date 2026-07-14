//! Real text shaping via [`cosmic_text`]. Caches one shaped `Buffer`
//! per [`TextCacheKey`] — every input that affects shaping (text hash,
//! font size, wrap width, line height, family, weight, halign, fit) —
//! so steady-state measurement is `HashMap` lookup only: no reshape,
//! no allocation. The cache is bounded:
//! [`CosmicMeasure::end_frame_evict`] drops the least-recently-shaped
//! buffers each frame (keeping every buffer a live layout entry still
//! references), so a continuous resize drag — every width unique, a
//! fresh entry per run per frame — stays bounded instead of growing
//! without limit.
//!
//! The render side reaches the cached `Buffer`s and the `FontSystem`
//! through [`CosmicMeasure::split_for_render`] (via
//! `TextShaper::with_render_split`) — a disjoint borrow, not a trait
//! object; `text/mod.rs` documents why there's no `TextMeasure` trait.
//!
//! Hash collisions are theoretically possible (we key on a 64-bit hash of the
//! text rather than storing the full string), but at typical UI scales the
//! cost of resolving them — verifying with the cached buffer's source string
//! on every hit — outweighs the cost of accepting the negligible risk.

use crate::common::hash::hash_str;
use crate::layout::types::align::HAlign;
use crate::primitives::size::Size;
use crate::text::{FontFamily, FontWeight, LineFit, MeasureResult, ShapeParams, TextCacheKey};
use cosmic_text::{
    Align as CosmicAlign, Attrs, Buffer, CacheKeyFlags, Family, FontSystem, Metrics, Shaping,
    Weight, fontdb,
};
use rustc_hash::{FxHashMap, FxHashSet};
use std::sync::Arc;

/// Bundled fonts shipped with the crate. Inter is the default UI /
/// proportional body font; JetBrains Mono is the monospace. Both ship as
/// a single variable-weight (`wght`) face, so Regular and Bold come from
/// one file each. Both are OFL 1.1. Weight is selected per-run via
/// [`FontWeight`] on the [`crate::TextStyle`], resolved in [`attrs_for`].
const INTER: &[u8] = include_bytes!("../../assets/fonts/Inter-VariableFont_opsz,wght.ttf");
const JBMONO: &[u8] = include_bytes!("../../assets/fonts/JetBrainsMono[wght].ttf");

const MAX_W_NONE: u32 = u32::MAX;

/// Cap on [`CosmicMeasure::ellipsis_cache`] entries. The cache keys on
/// `(quantized size, family, weight)` — a handful in normal use, but
/// unbounded under a continuous font-size zoom. Cleared wholesale past
/// this; a miss is one cheap "…" shape, so the occasional reset is
/// negligible.
pub(crate) const ELLIPSIS_CACHE_CAP: usize = 128;

fn quantize(v: f32) -> u32 {
    (v.max(0.0) * 64.0).round() as u32
}

fn key_for(text_hash: u64, params: ShapeParams, fit: LineFit) -> TextCacheKey {
    let ShapeParams {
        font_size_px,
        line_height_px,
        max_width_px,
        family,
        weight,
        halign,
    } = params;
    // Avoid colliding with INVALID. Probability astronomically low; map zero
    // to one so the renderer never silently drops a real run.
    let text_hash = text_hash.max(1);
    // Halign discriminates the key only when it feeds shaping: a
    // finite wrap width under the `Wrap` fit, where cosmic bakes
    // per-line offsets into the buffer. Unbounded shapes and the
    // truncated single-line fits (`Clip`/`Ellipsis` shape with
    // alignment `None` — the encoder owns placement) produce
    // identical buffers for every halign, so fold `halign_q` to
    // `Auto`'s discriminant (0) there — callers don't pay an N-way
    // cache split for identical glyph positions.
    let halign_q = match (max_width_px, fit) {
        (Some(_), LineFit::Wrap) => halign as u8,
        _ => HAlign::Auto as u8,
    };
    TextCacheKey {
        text_hash,
        size_q: quantize(font_size_px),
        max_w_q: max_width_px.map(quantize).unwrap_or(MAX_W_NONE),
        lh_q: quantize(line_height_px),
        family_q: family as u8,
        weight_q: weight as u8,
        halign_q,
        fit_q: fit as u8,
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

struct CacheEntry {
    /// Shaped buffer. Looked up by [`TextCacheKey`] at render time so the
    /// text backend can build a `TextArea` without reshaping.
    buffer: Buffer,
    measured: Size,
    /// Width of the widest unbreakable run, in logical px. Computed once on
    /// insert from the unbounded shaping result and reused for every later
    /// `measure` call that hits this entry.
    intrinsic_min: f32,
    /// Frame generation at the last measure-time touch (insert or
    /// `cache_hit`). The LRU recency key for [`CosmicMeasure::end_frame_evict`].
    last_used: u64,
}

/// Real-shaping text measurer. Owns a [`FontSystem`] populated by
/// [`CosmicMeasure::with_bundled_fonts`] (Inter + JetBrains Mono) and
/// a cache of shaped `Buffer`s keyed on the inputs that affect shaping.
/// Per-call font family + weight selection comes from [`FontFamily`] /
/// [`FontWeight`] on each [`Self::measure`] invocation; the named lookups
/// in [`attrs_for`] resolve against the bundled set.
pub struct CosmicMeasure {
    font_system: FontSystem,
    cache: FxHashMap<TextCacheKey, CacheEntry>,
    /// Monotonic frame counter, advanced once per frame by
    /// [`Self::advance_frame`]. Stamped onto each entry's `last_used` on
    /// every measure-time touch so eviction can drop the
    /// least-recently-shaped unpinned buffers.
    frame_gen: u64,
    /// Reusable scratch holding the `last_used` of every unpinned entry
    /// during [`Self::end_frame_evict`] — kept across frames so the
    /// (infrequent) eviction pass allocates nothing.
    evict_scratch: Vec<u64>,
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
    /// across machines. Per-call family + weight selection comes from
    /// [`FontFamily`] / [`FontWeight`] on each [`Self::measure`] invocation.
    pub fn with_bundled_fonts() -> Self {
        let sources = [INTER, JBMONO]
            .into_iter()
            .map(|b| fontdb::Source::Binary(Arc::new(b)));
        let font_system = FontSystem::new_with_fonts(sources);
        Self {
            font_system,
            cache: FxHashMap::default(),
            frame_gen: 0,
            evict_scratch: Vec::new(),
            ellipsis_cache: FxHashMap::default(),
            truncate_scratch: String::new(),
        }
    }

    /// Look up the shaped buffer for `key`. Returns `None` for keys that
    /// were never measured this `CosmicMeasure` instance — including
    /// [`TextCacheKey::INVALID`].
    pub(crate) fn buffer_for(&self, key: TextCacheKey) -> Option<&Buffer> {
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
    cache: &'a FxHashMap<TextCacheKey, CacheEntry>,
}

impl<'a> BufferLookup<'a> {
    pub fn get(&self, key: TextCacheKey) -> Option<&'a Buffer> {
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
    pub(crate) fn measure(&mut self, text: &str, params: ShapeParams) -> MeasureResult {
        self.measure_hashed(text, hash_str(text), params)
    }

    #[profiling::function]
    pub(crate) fn measure_hashed(
        &mut self,
        text: &str,
        text_hash: u64,
        params: ShapeParams,
    ) -> MeasureResult {
        let ShapeParams {
            font_size_px,
            line_height_px,
            max_width_px,
            family,
            weight,
            halign,
        } = params;
        if text.is_empty() || font_size_px <= 0.0 {
            return MeasureResult::INVALID;
        }
        let key = key_for(text_hash, params, LineFit::Wrap);
        if let Some(hit) = self.cache_hit(key) {
            return hit;
        }

        let metrics = Metrics::new(font_size_px, line_height_px);
        let mut buffer = Buffer::new(&mut self.font_system, metrics);
        buffer.set_size(max_width_px, None);
        // Per-line alignment travels through cosmic's `set_text`
        // `alignment` slot — that's the canonical entry point and
        // applies the align to every parsed buffer line in one
        // shot. Iterating `buffer.lines.iter_mut().set_align` after
        // `set_text` is the older API surface and tends to no-op on
        // freshly populated lines in 0.18+. Per-line align is only
        // meaningful with a finite wrap target (cosmic uses it as the
        // line width); without one we pass `None` so single-line
        // editors keep their widget-side `dx` placement.
        let alignment = max_width_px.and_then(|_| cosmic_align(halign));
        buffer.set_text(
            text,
            &attrs_for(family, weight),
            Shaping::Advanced,
            alignment,
        );
        buffer.shape_until_scroll(&mut self.font_system, false);

        let extent = shaped_extent(&buffer);
        self.cache.insert(
            key,
            CacheEntry {
                buffer,
                measured: extent.size,
                intrinsic_min: extent.intrinsic_min,
                last_used: self.frame_gen,
            },
        );
        MeasureResult {
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
    pub(crate) fn measure_truncated(
        &mut self,
        text: &str,
        params: ShapeParams,
        fit: LineFit,
        unbounded_key: TextCacheKey,
    ) -> MeasureResult {
        let ShapeParams {
            font_size_px,
            line_height_px,
            max_width_px,
            family,
            weight,
            // A truncated single line is positioned/aligned by the encoder,
            // not shaped with a per-line align.
            halign: _,
        } = params;
        let w = max_width_px.expect("measure_truncated requires a finite width");
        assert!(
            matches!(fit, LineFit::Clip | LineFit::Ellipsis),
            "measure_truncated requires Clip or Ellipsis",
        );
        if text.is_empty() || font_size_px <= 0.0 {
            return MeasureResult::INVALID;
        }
        let key = TextCacheKey {
            max_w_q: quantize(w),
            halign_q: HAlign::Auto as u8,
            fit_q: fit as u8,
            ..unbounded_key
        };
        if let Some(hit) = self.cache_hit(key) {
            return hit;
        }
        let metrics = Metrics::new(font_size_px, line_height_px);
        let attrs = attrs_for(family, weight);
        let probe = &self
            .cache
            .get(&unbounded_key)
            .expect("truncation requires the cached unbounded shape")
            .buffer;
        let line_w = first_line_right(probe);
        let multiline = probe.layout_runs().nth(1).is_some();

        let truncated = if line_w <= w && !multiline {
            false
        } else {
            // Reserve the ellipsis width only when we'll append one; a plain
            // clip cuts flush to the full available width.
            let mut append_ellipsis = false;
            let avail = if matches!(fit, LineFit::Ellipsis) {
                let ellipsis_w = self.ellipsis_advance(metrics, family, weight);
                append_ellipsis = ellipsis_w <= w;
                (w - ellipsis_w).max(0.0)
            } else {
                w
            };
            let mut cut = 0usize;
            let probe = &self
                .cache
                .get(&unbounded_key)
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
            self.truncate_scratch.push_str(text[..cut].trim_end());
            if append_ellipsis {
                self.truncate_scratch.push('…');
            }
            true
        };

        // Shape unbounded on one line: the cut already fit it to `w`, and the
        // encoder owns single-line placement. Binding to `Some(w)` + align
        // would measure the aligned glyph position, inflating a fits-anyway
        // label toward the box width.
        let mut buffer = Buffer::new(&mut self.font_system, metrics);
        buffer.set_size(None, None);
        let shaped_text = if truncated {
            self.truncate_scratch.as_str()
        } else {
            text
        };
        buffer.set_text(shaped_text, &attrs, Shaping::Advanced, None);
        buffer.shape_until_scroll(&mut self.font_system, false);

        let measured = shaped_extent(&buffer).size;
        self.cache.insert(
            key,
            CacheEntry {
                buffer,
                measured,
                intrinsic_min: 0.0,
                last_used: self.frame_gen,
            },
        );
        MeasureResult {
            size: measured,
            key,
            intrinsic_min: 0.0,
        }
    }

    /// Trailing advance of "…" at `metrics`/`family`/`weight`, memoized per
    /// `(quantized size, family, weight)`. The width is constant for a given
    /// size + face, so this is a map lookup after the first shape. The
    /// rare miss shapes into a throwaway buffer so the cached unbounded
    /// probe remains immutable.
    fn ellipsis_advance(
        &mut self,
        metrics: Metrics,
        family: FontFamily,
        weight: FontWeight,
    ) -> f32 {
        let key = (quantize(metrics.font_size), family as u8, weight as u8);
        if let Some(&w) = self.ellipsis_cache.get(&key) {
            return w;
        }
        let mut buffer = Buffer::new(&mut self.font_system, metrics);
        buffer.set_size(None, None);
        buffer.set_text("…", &attrs_for(family, weight), Shaping::Advanced, None);
        buffer.shape_until_scroll(&mut self.font_system, false);
        let w = first_line_right(&buffer);
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

    /// A cached entry's `MeasureResult` for `key`, or `None` on a miss.
    /// Refreshes the entry's `last_used` so a hit counts as recent for
    /// eviction — a buffer reused on a multi-size rotation must not age
    /// out as if it were a one-shot drag orphan.
    fn cache_hit(&mut self, key: TextCacheKey) -> Option<MeasureResult> {
        let now = self.frame_gen;
        self.cache.get_mut(&key).map(|entry| {
            entry.last_used = now;
            MeasureResult {
                size: entry.measured,
                key,
                intrinsic_min: entry.intrinsic_min,
            }
        })
    }

    /// `true` when the cache holds more than `max_keep` buffers — the
    /// cheap pre-gate [`crate::text::ShaperInner::end_frame`] checks
    /// before building the (O(reuse)) pin set, so the per-frame pin
    /// rebuild only happens when there is actually something to evict.
    pub(crate) fn over_budget(&self, max_keep: usize) -> bool {
        self.cache.len() > max_keep
    }

    /// Advance the frame generation. Called once per frame (eviction or
    /// not) so `last_used` stamps from different frames stay ordered —
    /// the LRU recency signal `end_frame_evict` reads.
    pub(crate) fn advance_frame(&mut self) {
        self.frame_gen = self.frame_gen.wrapping_add(1);
    }

    /// Repack-free eviction run from [`crate::text::ShaperInner::end_frame`]
    /// when the cache is over budget. `pinned` is the set of keys
    /// referenced by a live `reuse` entry this frame — exactly the keys
    /// the renderer can ask for — so they are never evicted regardless of
    /// recency. Among the *unpinned* remainder (stale rotation widths,
    /// drag orphans), keep at most `keep_unpinned` by `last_used` recency
    /// and drop the rest. Bounds the cache on a continuous resize drag
    /// (every width unique → a fresh orphan per run per frame) without
    /// touching the working set of a bounded multi-size rotation, whose
    /// unpinned widths stay under the budget and keep hitting.
    pub(crate) fn end_frame_evict(
        &mut self,
        pinned: &FxHashSet<TextCacheKey>,
        keep_unpinned: usize,
    ) {
        if self.cache.len() > pinned.len() + keep_unpinned {
            self.evict_scratch.clear();
            self.evict_scratch.extend(
                self.cache
                    .iter()
                    .filter(|(k, _)| !pinned.contains(*k))
                    .map(|(_, e)| e.last_used),
            );
            if self.evict_scratch.len() > keep_unpinned {
                // Cutoff = the `keep_unpinned`-th largest `last_used`;
                // keep entries at or above it. Ties at the cutoff retain
                // a few extra — harmless slack, not unbounded.
                let cut = self.evict_scratch.len() - keep_unpinned;
                let (_, &mut cutoff, _) = self.evict_scratch.select_nth_unstable(cut);
                self.cache
                    .retain(|k, e| pinned.contains(k) || e.last_used >= cutoff);
            }
        }
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
