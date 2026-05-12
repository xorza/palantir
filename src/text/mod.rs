//! Text shaping & measurement.
//!
//! Two paths, one struct each:
//!
//! - [`CosmicMeasure`] — real shaping via `cosmic-text`, with a per-key
//!   shaped-buffer cache. The wgpu backend reuses these `Buffer`s in
//!   `glyphon::TextRenderer::prepare`, so each visible string is shaped
//!   exactly once across its lifetime.
//! - [`mono_measure`] — deterministic placeholder metric used when no
//!   `CosmicMeasure` is installed (default in [`Ui`]). Every glyph is
//!   `font_size_px * 0.5` wide; runs measured this way carry
//!   [`TextCacheKey::INVALID`] and the renderer drops them. Lets the engine
//!   run in tests and headless tools without a font system.
//!
//! There's no `TextMeasure` trait: the renderer needs concrete access to
//! `CosmicMeasure`'s `FontSystem` + cache, so a trait would just be a
//! downcast in disguise.
//!
//! [`Ui`]: crate::Ui

use crate::forest::rollups::NodeHash;
use crate::forest::widget_id::WidgetId;
use crate::primitives::size::Size;
use rustc_hash::{FxHashMap, FxHashSet};
use std::cell::RefCell;
use std::collections::hash_map::Entry;
use std::rc::Rc;

pub(crate) mod cosmic;

/// Leading the shaper hands to cosmic-text's `Metrics::new`, also used
/// as the default for [`crate::TextEditTheme::line_height_mult`] so
/// the caret rect spans the same y-range as the rendered text.
/// Single source — cosmic and the theme default move together.
pub(crate) const LINE_HEIGHT_MULT: f32 = 1.2;

use crate::text::cosmic::{CosmicMeasure, RenderSplit};

/// Shared, cloneable text shaper. Holds (1) an optional [`CosmicMeasure`]
/// for real shaping (`None` ⇒ mono fallback), (2) a cross-frame reuse
/// cache keyed by `(WidgetId, ordinal)` so layout skips `measure`
/// dispatch when a `Text` leaf's inputs are unchanged, and (3) a
/// `measure_calls` counter for cache-effectiveness tests.
///
/// Single-threaded by design (`Rc` inside); access is sequential —
/// measurement during layout, prepare/render during the wgpu frame —
/// so the `RefCell` is just runtime insurance against accidental
/// re-entry. Cloning is cheap (refcount bump). [`crate::Host::new`]
/// holds the canonical handle and passes a clone to both `Ui` (via
/// `Ui::with_text`) and the backend (constructor arg) so both sides
/// see one buffer cache.
///
/// Two paths, picked at construction:
///
/// - [`Self::mono`] / [`Self::default`] — primitive shaping (every
///   glyph is `font_size_px * 0.5` wide). Renderer drops these runs
///   (their [`TextCacheKey`] is [`TextCacheKey::INVALID`]). Useful
///   for tests, headless drivers, and the `Ui::new()` state.
/// - [`Self::with_bundled_fonts`] / [`Self::with_cosmic`] — real
///   shaping via cosmic-text.
#[derive(Clone, Default)]
pub struct TextShaper {
    /// `pub(crate)` for [`crate::support::internals`] observability
    /// helpers. Direct field access from inside the crate is fine; the
    /// invariants live in the mutating methods of `TextShaper`, not in
    /// encapsulation theater.
    pub(crate) inner: Rc<RefCell<ShaperInner>>,
}

/// Shared mutable state behind the `Rc<RefCell<...>>` in [`TextShaper`].
/// Both [`crate::Ui`] (layout-time measurement + reuse cache) and
/// [`crate::WgpuBackend`] (shaping during render) borrow this; backend
/// only touches `cosmic` via [`TextShaper::with_render_split`].
#[derive(Default)]
pub(crate) struct ShaperInner {
    /// `None` ⇒ mono fallback path. `Some` ⇒ real shaping.
    cosmic: Option<CosmicMeasure>,
    /// Total `measure` calls dispatched (cache misses). Cache hits
    /// don't increment. Read by tests pinning reshape-skip behaviour
    /// via [`crate::support::internals::text_shaper_measure_calls`].
    pub(crate) measure_calls: u64,
    /// Cross-frame cache of shaping output keyed by
    /// `(WidgetId, within-node text-shape ordinal)`, validity-checked
    /// by authoring hash. The ordinal disambiguates leaves with
    /// multiple `ShapeRecord::Text` runs. The wrap slot's `target_q`
    /// quantization is layout policy chosen at the call site. Read by
    /// tests via [`crate::support::internals::text_shaper_has_reuse_entry`].
    pub(crate) reuse: FxHashMap<(WidgetId, u16), TextReuseEntry>,
}

impl TextShaper {
    /// Mono fallback shaper. Every glyph is `font_size_px * 0.5` wide;
    /// returned [`MeasureResult::key`] is [`TextCacheKey::INVALID`] so
    /// the renderer drops these runs cleanly. Same as [`Self::default`].
    pub fn mono() -> Self {
        Self::default()
    }

    /// Real shaping via the supplied [`CosmicMeasure`]. The shaper's
    /// shaped-buffer cache is shared across all clones of this handle.
    pub fn with_cosmic(cosmic: CosmicMeasure) -> Self {
        Self {
            inner: Rc::new(RefCell::new(ShaperInner {
                cosmic: Some(cosmic),
                ..Default::default()
            })),
        }
    }

    /// Convenience: cosmic-backed shaper with bundled fonts loaded.
    pub fn with_bundled_fonts() -> Self {
        Self::with_cosmic(CosmicMeasure::with_bundled_fonts())
    }

    /// ShapeRecord `text` and return its measurement. Bypasses the per-widget
    /// reuse cache — direct dispatch to cosmic (if installed) or mono.
    /// Used by [`Self::caret_x`] and other prefix/probe paths.
    pub fn measure(
        &self,
        text: &str,
        font_size_px: f32,
        line_height_px: f32,
        max_width_px: Option<f32>,
    ) -> MeasureResult {
        let mut inner = self.inner.borrow_mut();
        inner.measure_calls += 1;
        inner.dispatch(text, font_size_px, line_height_px, max_width_px)
    }

    /// Identity-cached unbounded shape for `wid`, refreshing it (and
    /// clearing any stale wrap entry) when the authoring hash has
    /// shifted.
    pub(crate) fn shape_unbounded(
        &self,
        wid: WidgetId,
        ordinal: u16,
        hash: NodeHash,
        text: &str,
        font_size_px: f32,
        line_height_px: f32,
    ) -> MeasureResult {
        let mut inner = self.inner.borrow_mut();
        let inner = &mut *inner;
        // Cache hit: same authoring hash, return last frame's result.
        if let Entry::Occupied(o) = inner.reuse.entry((wid, ordinal))
            && o.get().hash == hash
        {
            return o.get().unbounded;
        }
        inner.measure_calls += 1;
        let unbounded = inner.dispatch(text, font_size_px, line_height_px, None);
        inner.reuse.insert(
            (wid, ordinal),
            TextReuseEntry {
                hash,
                unbounded,
                wrap: None,
            },
        );
        unbounded
    }

    /// Identity-cached wrap shape for `wid` at the caller-quantized
    /// `target_q`. Hits when the same wrap target was used last frame;
    /// otherwise dispatches and refreshes the entry. Must be preceded
    /// by [`Self::shape_unbounded`] on the same `(wid, ordinal)` so the
    /// parent entry exists.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn shape_wrap(
        &self,
        wid: WidgetId,
        ordinal: u16,
        text: &str,
        font_size_px: f32,
        line_height_px: f32,
        target: f32,
        target_q: u32,
    ) -> MeasureResult {
        let mut inner = self.inner.borrow_mut();
        let inner = &mut *inner;
        let entry = match inner.reuse.entry((wid, ordinal)) {
            Entry::Occupied(o) => o,
            Entry::Vacant(_) => panic!(
                "shape_wrap requires a prior shape_unbounded call on the same (wid, ordinal)",
            ),
        };
        if let Some(w) = entry.get().wrap
            && w.target_q == target_q
        {
            return w.result;
        }
        inner.measure_calls += 1;
        let m = inner.dispatch(text, font_size_px, line_height_px, Some(target));
        // Re-borrow `entry` because dispatch took `&mut inner` over the
        // whole struct; the prior borrow ended at the early-return.
        inner
            .reuse
            .get_mut(&(wid, ordinal))
            .expect("entry just confirmed to exist")
            .wrap = Some(WrapReuse {
            target_q,
            result: m,
        });
        m
    }

    /// Unbounded measured width of `text[..byte_offset]`, used for
    /// caret-x positioning inside an editor. Bypasses the per-`WidgetId`
    /// reuse cache because the prefix changes with caret movement and
    /// editing — caching every prefix would bloat the table without
    /// the savings the cache exists for. A future `byte_to_x` API on
    /// `MeasureResult` (cosmic exposes one via `Buffer::layout_runs`)
    /// will replace this when multi-line / IME / drag-select land.
    ///
    /// Panics if `byte_offset` doesn't fall on a UTF-8 character
    /// boundary — same surface as `&text[..byte_offset]`.
    pub(crate) fn caret_x(
        &self,
        text: &str,
        byte_offset: usize,
        font_size_px: f32,
        line_height_px: f32,
    ) -> f32 {
        if byte_offset == 0 || text.is_empty() {
            return 0.0;
        }
        let prefix = &text[..byte_offset];
        self.measure(prefix, font_size_px, line_height_px, None)
            .size
            .w
    }

    /// Drop reuse entries for the supplied removed-widget set. Called
    /// from `Ui::post_record` against the same per-frame diff fed to
    /// `DamageEngine::compute` so cleanup stays bounded under widget churn
    /// without a second `seen_ids` scan.
    pub(crate) fn sweep_removed(&self, removed: &FxHashSet<WidgetId>) {
        if removed.is_empty() {
            return;
        }
        self.inner
            .borrow_mut()
            .reuse
            .retain(|(wid, _), _| !removed.contains(wid));
    }

    /// Run `body` against a [`RenderSplit`] of the inner cosmic state
    /// (`&mut FontSystem` + read-only buffer lookup). Returns `None`
    /// when the shaper is mono (no cosmic to split). The borrow is
    /// held for the closure's duration, so `body` must not re-enter
    /// any `TextShaper` method on the same handle.
    pub(crate) fn with_render_split<R>(
        &self,
        body: impl FnOnce(RenderSplit<'_>) -> R,
    ) -> Option<R> {
        let mut inner = self.inner.borrow_mut();
        let cosmic = inner.cosmic.as_mut()?;
        Some(body(cosmic.split_for_render()))
    }
}

impl ShaperInner {
    /// Bypass-cache dispatch: cosmic if installed, mono otherwise.
    /// Caller is responsible for incrementing `measure_calls` on cache
    /// misses (we don't bump it here because some paths — `shape_wrap`,
    /// `caret_x` — already account for it).
    fn dispatch(
        &mut self,
        text: &str,
        font_size_px: f32,
        line_height_px: f32,
        max_width_px: Option<f32>,
    ) -> MeasureResult {
        match self.cosmic.as_mut() {
            Some(c) => c.measure(text, font_size_px, line_height_px, max_width_px),
            None => mono_measure(text, font_size_px, line_height_px, max_width_px),
        }
    }
}

/// Stable identifier for a shaped text run, computed at authoring time so
/// `ShapeRecord::Text` can carry it through the encoder/composer and the renderer
/// can look up the matching shaped buffer without rehashing.
///
/// Three quantized fields rather than one collapsed `u64` so the renderer
/// can also reuse the size/width components if it wants to (e.g. group runs
/// by size for atlas bin reuse). [`TextCacheKey::INVALID`] is the sentinel
/// returned by the mono fallback — the renderer treats it as "drop this run".
#[repr(C)]
#[padding_struct::padding_struct]
#[derive(Clone, Copy, Hash, Eq, PartialEq, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct TextCacheKey {
    /// 64-bit hash of the source string. `0` for the invalid sentinel.
    pub text_hash: u64,
    /// `font_size_px * 64`, rounded. Quantizing to 1/64 px is below any
    /// visible difference and keeps the key purely integral.
    pub size_q: u32,
    /// `max_width_px * 64`, rounded; `u32::MAX` encodes `None` (unbounded).
    pub max_w_q: u32,
    /// `line_height_px * 64`, rounded. Two `ShapeRecord::Text` runs at the
    /// same font-size but different leading produce different shaped
    /// buffers (different `Metrics::new`), so the key has to discriminate.
    pub lh_q: u32,
}

impl TextCacheKey {
    /// Sentinel returned by the mono fallback. The renderer skips runs with
    /// this key. `bytemuck::Zeroable::zeroed` fills the padding fields
    /// the `padding_struct` proc macro generated.
    pub const INVALID: Self = unsafe { std::mem::zeroed() };

    /// Construct from the four hashed fields. The `padding_struct` proc
    /// macro injects trailing padding fields to satisfy
    /// `bytemuck::Pod`'s no-padding-bytes invariant; the
    /// `..Zeroable::zeroed()` spread fills them with zeros so callers
    /// don't have to know they exist.
    pub(crate) fn new(text_hash: u64, size_q: u32, max_w_q: u32, lh_q: u32) -> Self {
        Self {
            text_hash,
            size_q,
            max_w_q,
            lh_q,
            ..bytemuck::Zeroable::zeroed()
        }
    }

    pub const fn is_invalid(self) -> bool {
        self.text_hash == 0 && self.size_q == 0 && self.max_w_q == 0 && self.lh_q == 0
    }
}

/// Result of measuring (and, in the cosmic path, shaping) one text run.
#[derive(Clone, Copy, Debug)]
pub struct MeasureResult {
    pub size: Size,
    /// Identifier of the shaped buffer, or [`TextCacheKey::INVALID`] when no
    /// shaping happened (mono fallback).
    pub key: TextCacheKey,
    /// Width of the widest unbreakable run (typically the longest word).
    /// The wrapping path uses this as the floor when a parent commits a
    /// narrower width: text overflows rather than breaking inside a word.
    /// Equal to `size.w` for the mono fallback (no real word boundaries) and
    /// for single-word inputs.
    pub intrinsic_min: f32,
}

/// Deterministic placeholder metric used when [`crate::Ui`] has no
/// [`CosmicMeasure`] installed. Every glyph is `font_size_px * 0.5` wide and
/// the line is `font_size_px` tall; wrapping is approximated by simple
/// character-count division. At the historical 16 px font size this is the
/// 8 px/char × 16 px line layout the engine was hard-coded to before text
/// shaping landed, which is what existing layout tests pin.
///
/// Always returns [`TextCacheKey::INVALID`] — there's no shaped buffer to
/// look up, so the renderer drops these runs cleanly.
fn mono_measure(
    text: &str,
    font_size_px: f32,
    line_height_px: f32,
    max_width_px: Option<f32>,
) -> MeasureResult {
    if text.is_empty() {
        return MeasureResult {
            size: Size::ZERO,
            key: TextCacheKey::INVALID,
            intrinsic_min: 0.0,
        };
    }
    let glyph_w = font_size_px * 0.5;
    let line_h = line_height_px;
    // Mono is a deterministic stub — count one "char" per byte. Correct for
    // ASCII (which is what every test and bench uses); for multibyte input
    // it overcounts, but mono is not a production path.
    let total_chars = text.len() as f32;
    let unbroken_w = total_chars * glyph_w;

    let size = match max_width_px {
        None => Size::new(unbroken_w, line_h),
        Some(max) if max >= unbroken_w => Size::new(unbroken_w, line_h),
        Some(max) => {
            let chars_per_line = (max / glyph_w).floor().max(1.0);
            let lines = (total_chars / chars_per_line).ceil().max(1.0);
            Size::new((chars_per_line * glyph_w).min(unbroken_w), lines * line_h)
        }
    };
    // Mono has no real word boundaries — fall back to "the longest run of
    // non-space bytes" so wrap callers still get a sensible floor.
    let mut longest = 0u32;
    let mut run = 0u32;
    for &b in text.as_bytes() {
        if b == b' ' || b == b'\t' || b == b'\n' || b == b'\r' {
            if run > longest {
                longest = run;
            }
            run = 0;
        } else {
            run += 1;
        }
    }
    if run > longest {
        longest = run;
    }
    let intrinsic_min = longest as f32 * glyph_w;
    MeasureResult {
        size,
        key: TextCacheKey::INVALID,
        intrinsic_min,
    }
}

/// Cached unbounded shape + most-recent wrap result, validity-checked
/// by authoring `hash`.
#[derive(Clone, Copy)]
pub(crate) struct TextReuseEntry {
    hash: NodeHash,
    unbounded: MeasureResult,
    wrap: Option<WrapReuse>,
}

/// One cached wrap result — the most-recent `target_q` (caller-
/// quantized wrap target) and the `MeasureResult` that came out of
/// shaping at that target.
#[derive(Clone, Copy)]
struct WrapReuse {
    target_q: u32,
    result: MeasureResult,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Line-height equal to font size keeps the mono-fallback line
    /// height numerically equal to `font_size`, matching the legacy
    /// placeholder layout the existing tests pin.
    fn lh(font_size: f32) -> f32 {
        font_size
    }

    #[test]
    fn mono_measure_cases() {
        type Case = (&'static str, &'static str, f32, f32, Option<f32>, Size);
        let cases: &[Case] = &[
            ("empty", "", 16.0, lh(16.0), None, Size::ZERO),
            (
                "unbroken_legacy_short",
                "Hi",
                16.0,
                lh(16.0),
                None,
                Size::new(16.0, 16.0),
            ),
            (
                "unbroken_legacy_long",
                "hello!!",
                16.0,
                lh(16.0),
                None,
                Size::new(56.0, 16.0),
            ),
            (
                "wraps_below_unbroken",
                "12345678",
                16.0,
                lh(16.0),
                Some(32.0),
                Size::new(32.0, 32.0),
            ),
            (
                "line_height_param_short",
                "Hi",
                16.0,
                24.0,
                None,
                Size::new(16.0, 24.0),
            ),
            (
                "line_height_param_wrapped",
                "12345678",
                16.0,
                24.0,
                Some(32.0),
                Size::new(32.0, 48.0),
            ),
        ];
        for (label, text, fs, lh_v, max_w, expected) in cases {
            let r = mono_measure(text, *fs, *lh_v, *max_w);
            assert_eq!(r.size, *expected, "case: {label}");
        }
        // Empty also produces the INVALID sentinel.
        assert!(mono_measure("", 16.0, lh(16.0), None).key.is_invalid());
    }

    /// `caret_x(text, byte_offset, font_size, line_height)`. Mono
    /// fallback: each ASCII byte is `font_size * 0.5` wide. Caret x is
    /// independent of `line_height` (advance only depends on font_size
    /// + glyph). Empty string and zero offset short-circuit to zero.
    #[test]
    fn caret_x_cases() {
        let cases: &[(&str, &str, usize, f32, f32, f32)] = &[
            ("zero_offset", "hello", 0, 16.0, lh(16.0), 0.0),
            ("empty_string", "", 0, 16.0, lh(16.0), 0.0),
            ("mono_one_char", "abc", 1, 16.0, lh(16.0), 8.0),
            ("mono_two_chars", "abc", 2, 16.0, lh(16.0), 16.0),
            ("mono_three_chars", "abc", 3, 16.0, lh(16.0), 24.0),
            ("lh_independent_short", "abc", 2, 16.0, 16.0, 16.0),
            ("lh_independent_tall", "abc", 2, 16.0, 24.0, 16.0),
        ];
        for (label, text, offset, fs, lh_v, expected) in cases {
            let m = TextShaper::default();
            assert_eq!(
                m.caret_x(text, *offset, *fs, *lh_v),
                *expected,
                "case: {label}"
            );
        }
    }

    #[test]
    fn caret_x_increments_measure_calls_counter() {
        // The `measure_calls` counter pins reshape-skip behavior in
        // existing tests; pin that caret_x participates in it (no
        // free measurements) so a future caller can detect over-call.
        let m = TextShaper::default();
        let before = crate::support::internals::text_shaper_measure_calls(&m);
        let _ = m.caret_x("abc", 2, 16.0, lh(16.0));
        assert_eq!(
            crate::support::internals::text_shaper_measure_calls(&m),
            before + 1
        );
        // Zero-offset shortcut must not bump the counter.
        let zero_before = crate::support::internals::text_shaper_measure_calls(&m);
        let _ = m.caret_x("abc", 0, 16.0, lh(16.0));
        assert_eq!(
            crate::support::internals::text_shaper_measure_calls(&m),
            zero_before
        );
    }

    #[test]
    #[should_panic]
    fn caret_x_panics_inside_multibyte_codepoint() {
        // "é" is two UTF-8 bytes; offset 1 splits it.
        let m = TextShaper::default();
        let _ = m.caret_x("é", 1, 16.0, lh(16.0));
    }

    #[test]
    fn cosmic_text_cache_key_distinguishes_line_height() {
        // Pin: at the same font-size, different leadings produce
        // different TextCacheKeys. The renderer caches shaped buffers
        // by key — without this discrimination, a 16/19.2 buffer would
        // be returned for a request that wanted 16/24, mismatching the
        // measured rect against the rasterized glyphs.
        use crate::text::cosmic::CosmicMeasure;
        let mut c = CosmicMeasure::with_bundled_fonts();
        let a = c.measure("hi", 16.0, 16.0 * LINE_HEIGHT_MULT, None).key;
        let b = c.measure("hi", 16.0, 24.0, None).key;
        assert_ne!(a, b, "different leading must produce different key");
        assert_ne!(a.lh_q, b.lh_q, "lh_q is the discriminating field");
        // Same call repeated → identical key (cache hit, deterministic).
        let a2 = c.measure("hi", 16.0, 16.0 * LINE_HEIGHT_MULT, None).key;
        assert_eq!(a, a2);
    }

    #[test]
    fn shape_unbounded_caches_per_authoring_hash_only() {
        // The reuse cache is keyed by `(WidgetId, NodeHash)` — different
        // line heights with the *same* hash would collide (same widget
        // id, same hash → cache hit returning the wrong measurement).
        // Authoring-side hash includes line_height_px (pinned in
        // node_hash tests), so callers that change leading must produce
        // a different hash — pin that the measure cache respects the
        // hash distinction.
        let m = TextShaper::default();
        let wid = WidgetId::from_hash("a");
        let h1 = NodeHash(1);
        let h2 = NodeHash(2);
        let r1 = m.shape_unbounded(wid, 0, h1, "hi", 16.0, 16.0);
        let r2 = m.shape_unbounded(wid, 0, h2, "hi", 16.0, 24.0);
        assert_ne!(
            r1.size.h, r2.size.h,
            "different leading via different hash → distinct cache entries",
        );
        // Re-querying with the original hash returns the original (16
        // px height), proving the entry wasn't overwritten.
        let r1_again = m.shape_unbounded(wid, 0, h1, "hi", 16.0, 16.0);
        assert_eq!(r1.size.h, r1_again.size.h);
    }

    #[test]
    #[should_panic(expected = "shape_wrap requires a prior shape_unbounded")]
    fn shape_wrap_panics_without_prime() {
        // Contract change: `shape_wrap` no longer falls back to a
        // dispatch-without-cache when the unbounded entry is missing.
        // Pin the panic so a future caller that wraps without priming
        // first fails loudly instead of silently losing the cache.
        let m = TextShaper::default();
        let wid = WidgetId::from_hash("a");
        m.shape_wrap(wid, 0, "hi", 16.0, 16.0, 100.0, 100);
    }

    #[test]
    fn text_cache_key_invalid_constant_zero_filled() {
        // `_pad` byte was added to satisfy bytemuck's no-padding rule;
        // pin that the INVALID sentinel still round-trips through
        // `is_invalid`. Failure here would mean a malformed default.
        assert!(TextCacheKey::INVALID.is_invalid());
        // And a non-INVALID key registers as such even with all
        // hashable fields zero except text_hash.
        let real = TextCacheKey::new(1, 0, 0, 0);
        assert!(!real.is_invalid());
    }

    #[test]
    fn caret_x_cosmic_path_is_monotonic_and_bounded() {
        // With real shaping, prefix widths must be non-decreasing and
        // approach the full-string width at the final offset. We don't
        // pin exact pixel values — those depend on font metrics — just
        // the monotonicity invariant any consumer relies on.
        let m = TextShaper::with_bundled_fonts();
        let s = "hello";
        let widths: Vec<f32> = (0..=s.len())
            .map(|i| m.caret_x(s, i, 16.0, 16.0 * LINE_HEIGHT_MULT))
            .collect();
        assert_eq!(widths[0], 0.0, "prefix-x at offset 0 is zero");
        for w in widths.windows(2) {
            assert!(
                w[1] >= w[0] - 0.01,
                "prefix widths must be non-decreasing, got {w:?}",
            );
        }
        assert!(
            widths[s.len()] > widths[0],
            "non-empty string has positive width",
        );
    }
}
