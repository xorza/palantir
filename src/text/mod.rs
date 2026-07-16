//! Text shaping & measurement.
//!
//! Two paths, one struct each:
//!
//! - [`CosmicMeasure`] — real shaping via `cosmic-text`, with a per-key
//!   shaped-buffer cache. The wgpu backend reuses these `Buffer`s in its
//!   text prepare/append path. The frontend reconstructs an evicted entry
//!   from its retained text shape before emitting the compact cache key.
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

use crate::common::content_hash::ContentHash;
use crate::layout::types::align::{Align, HAlign, VAlign};
use crate::primitives::rect::Rect;
use crate::primitives::size::Size;
use crate::primitives::widget_id::WidgetId;
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

/// Additive step on the text-scale ladder used by the composer to snap
/// continuous zoom scales to discrete glyph-cache keys (`composer::
/// snap_text_scale`). The cascade computes text damage rects at the
/// unscaled cascade scale; the composer paints glyphs at the snapped
/// scale — between rungs the painted block can be up to
/// `TEXT_SCALE_STEP / 2` wider than the damage rect on each axis.
/// [`crate::forest::shapes::record::text_paint_bbox_local`] inflates
/// by this fraction to keep damage covering the worst-case painted
/// extent.
///
/// Single source — `composer::TEXT_SCALE_STEP` re-exports this value.
pub(crate) const TEXT_SCALE_STEP: f32 = 0.005;

use crate::text::cosmic::{CosmicMeasure, RenderSplit};

/// Output buffer for [`TextShaper::selection_rects`]. Stack-allocated
/// for selections up to 16 visual lines; spills to heap for larger.
/// 16 covers the typical use case (single-line input, few-line wrapped
/// label) without alloc; rare multi-line editor selections beyond
/// that pay one heap alloc per paint.
pub(crate) type SelectionRects = tinyvec::TinyVec<[Rect; 16]>;

/// Font family picker on [`crate::TextStyle`] and
/// [`crate::Shape::Text`]. `Sans` resolves to bundled Inter (the default
/// proportional face); `Mono` resolves to bundled JetBrains Mono. Both
/// ship inside [`CosmicMeasure::with_bundled_fonts`]; the mono-fallback
/// shaper (when no `CosmicMeasure` is installed) ignores family entirely.
/// Weight (Regular/Bold) is an independent axis — see [`FontWeight`].
///
/// `#[repr(u8)]` with explicit discriminants pins the on-disk tag so
/// `TextCacheKey::family_q` and the `ShapeRecord::Text` hash byte
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

/// Font weight picker on [`crate::TextStyle`] and [`crate::Shape::Text`],
/// independent of [`FontFamily`]. `Regular` shapes with the family's
/// normal face; `Bold` requests the bold face (a distinct static face
/// for Inter, an instantiated `wght` for the variable JetBrains
/// Mono) via cosmic-text's `Attrs::weight` in [`attrs_for`].
///
/// `#[repr(u8)]` pins the tag for `TextCacheKey::weight_q` and the
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

/// Shared, cloneable text shaper. Holds (1) an optional [`CosmicMeasure`]
/// for real shaping (`None` ⇒ mono fallback), (2) a cross-frame reuse
/// cache keyed by `(WidgetId, ordinal)` so layout skips `measure`
/// dispatch when a `Text` leaf's inputs are unchanged, and (3) a
/// `measure_calls` counter for cache-effectiveness tests.
///
/// Single-threaded by design (`Rc` inside); access is sequential —
/// measurement during layout, prepare/render during the wgpu frame —
/// so the `RefCell` is just runtime insurance against accidental
/// re-entry. Cloning is cheap (refcount bump). [`crate::WindowRenderer::new`]
/// holds the canonical handle and passes a clone to both `Ui` (via
/// `Ui::with_text`) and the backend (constructor arg) so both sides
/// see one buffer cache.
///
/// Two paths, picked at construction:
///
/// - [`Self::mono`] / [`Self::default`] — primitive shaping (every
///   glyph is `font_size_px * 0.5` wide). WindowRenderer drops these runs
///   (their [`TextCacheKey`] is [`TextCacheKey::INVALID`]). Useful
///   for tests, headless drivers, and the `Ui::for_test()` state.
/// - [`Self::with_bundled_fonts`] / [`Self::with_cosmic`] — real
///   shaping via cosmic-text.
#[derive(Clone, Debug, Default)]
pub struct TextShaper {
    /// `pub(crate)` for [`test_support`] observability helpers. Direct
    /// field access from inside the crate is fine; invariants live in
    /// the mutating methods of `TextShaper`, not in encapsulation theater.
    pub(crate) inner: Rc<RefCell<ShaperInner>>,
}

/// Shared mutable state behind the `Rc<RefCell<...>>` in [`TextShaper`].
/// Both [`crate::Ui`] (layout-time measurement + reuse cache) and
/// [`crate::WgpuBackend`] (shaping during render) borrow this; backend
/// only touches `cosmic` via [`TextShaper::with_render_split`].
#[derive(Debug, Default)]
pub(crate) struct ShaperInner {
    /// `None` ⇒ mono fallback path. `Some` ⇒ real shaping.
    cosmic: Option<CosmicMeasure>,
    /// Total shaping dispatches: reuse-cache misses in
    /// `shape_unbounded` / `shape_wrap`, plus every bypass
    /// [`TextShaper::measure`] call — which may still hit the cosmic
    /// buffer cache, so this counts dispatches, not reshapes. Reuse-
    /// cache hits don't increment. Read by tests pinning reshape-skip
    /// behaviour via [`test_support::measure_calls`].
    pub(crate) measure_calls: u64,
    /// Cross-frame cache of shaping output keyed by
    /// `(WidgetId, within-node text-shape ordinal)`, validity-checked
    /// by authoring hash. The ordinal disambiguates leaves with
    /// multiple `ShapeRecord::Text` runs. The wrap slot's `target_q`
    /// quantization is layout policy chosen at the call site. Read by
    /// tests via [`test_support::has_reuse_entry`].
    pub(crate) reuse: FxHashMap<(WidgetId, u16), TextReuseEntry>,
}

/// Max cosmic buffers retained after per-frame maintenance. Missing entries
/// are restored from retained text shapes at encode, so the cache needs no
/// separate live-layout allowance.
const BUFFER_BUDGET: usize = 2048;

/// Bundled text-shaping parameters, threaded through the `TextShaper` /
/// `CosmicMeasure` query surface so every call carries one value instead
/// of the same loose args (font metrics + wrap width + family + weight +
/// per-line alignment). `max_width_px` is the wrap/truncation width
/// (`None` = unbounded); `halign` aligns each line inside that width
/// (ignored when unbounded, as in `shape_unbounded`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShapeParams {
    pub font_size_px: f32,
    pub line_height_px: f32,
    pub max_width_px: Option<f32>,
    pub family: FontFamily,
    pub weight: FontWeight,
    pub halign: HAlign,
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
    /// Used by [`Self::cursor_xy`] and other shape/probe paths.
    pub fn measure(&self, text: &str, params: ShapeParams) -> MeasureResult {
        let mut inner = self.inner.borrow_mut();
        inner.measure_calls += 1;
        inner.dispatch_direct(text, params)
    }

    /// Identity-cached unbounded shape for `wid`, refreshing it (and
    /// clearing any stale wrap entry) when the authoring hash has
    /// shifted.
    pub(crate) fn shape_unbounded(
        &self,
        wid: WidgetId,
        ordinal: u16,
        hash: ContentHash,
        text: &str,
        text_hash: u64,
        params: ShapeParams,
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
        // Unbounded shape ignores `halign` — cosmic only does per-line
        // alignment when there's a wrap target to align inside, and
        // there's no width here. Always passes `HAlign::Auto` so the
        // shaped buffer (and its `TextCacheKey`) match callers who
        // look it up without an align param.
        let unbounded = dispatch(
            &mut inner.cosmic,
            text,
            text_hash,
            ShapeParams {
                max_width_px: None,
                halign: HAlign::Auto,
                ..params
            },
            LineFit::Wrap,
            TextCacheKey::INVALID,
        );
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

    /// Identity-cached width-bounded shape for `wid` at the caller-
    /// quantized `target_q`. `fit` picks the overflow behaviour: `Wrap`
    /// reflows to the target width, `Clip` hard-cuts to one line, `Ellipsis`
    /// cuts to one line with a trailing `…`. Hits when the same target +
    /// halign + mode was used last frame; otherwise dispatches and refreshes
    /// the entry. Must be preceded by [`Self::shape_unbounded`] on the same
    /// `(wid, ordinal)` this frame so the parent entry exists and is fresh —
    /// checked against `hash`, the same authoring hash the unbounded call
    /// validated the entry with.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn shape_wrap(
        &self,
        wid: WidgetId,
        ordinal: u16,
        hash: ContentHash,
        text: &str,
        params: ShapeParams,
        target_q: u32,
        fit: LineFit,
    ) -> MeasureResult {
        let halign = params.halign;
        let mut inner = self.inner.borrow_mut();
        let ShaperInner {
            cosmic,
            measure_calls,
            reuse,
            ..
        } = &mut *inner;
        let mut entry = match reuse.entry((wid, ordinal)) {
            Entry::Occupied(o) => o,
            Entry::Vacant(_) => panic!(
                "shape_wrap requires a prior shape_unbounded call on the same (wid, ordinal)",
            ),
        };
        debug_assert!(
            entry.get().hash == hash,
            "shape_wrap on a stale entry — shape_unbounded must run first with the current hash",
        );
        if let Some(w) = entry.get().wrap
            && w.target_q == target_q
            && w.halign == halign
            && w.fit == fit
        {
            return w.result;
        }
        let unbounded_key = entry.get().unbounded.key;
        *measure_calls += 1;
        let m = dispatch(
            cosmic,
            text,
            unbounded_key.text_hash,
            params,
            fit,
            unbounded_key,
        );
        entry.get_mut().wrap = Some(WrapReuse {
            target_q,
            halign,
            fit,
            result: m,
        });
        m
    }

    /// Borrow the shaped cosmic `Buffer` for `(text, fs, lh, mw)`,
    /// shaping on demand if the cache misses. Returns `None` on the
    /// mono fallback (no cosmic installed) or empty text (cosmic
    /// returns the invalid sentinel key). Centralises the
    /// `measure → borrow → cosmic → buffer_for` preamble for every
    /// caret/selection helper below.
    fn with_buffer<R>(
        &self,
        text: &str,
        params: ShapeParams,
        body: impl FnOnce(&cosmic_text::Buffer) -> R,
    ) -> Option<R> {
        let mut inner = self.inner.borrow_mut();
        inner.measure_calls += 1;
        let result = inner.dispatch_direct(text, params);
        let buffer = inner.cosmic.as_ref()?.buffer_for(result.key)?;
        Some(body(buffer))
    }

    /// (x, y_top, line_height) for the caret at `byte_offset` inside
    /// `text` rendered at `(font_size_px, line_height_px)` with an
    /// optional wrap `max_width_px`. Multi-line aware via cosmic-text
    /// layout runs (each `\n` and each soft-wrap segment becomes a
    /// distinct visual line). Mono fallback / empty-text path
    /// collapses to a 1D layout — `y_top = 0`, `x` from a flat mono
    /// per-byte estimate — usable for tests / headless.
    pub(crate) fn cursor_xy(
        &self,
        text: &str,
        byte_offset: usize,
        params: ShapeParams,
    ) -> CursorPos {
        let ShapeParams {
            font_size_px,
            line_height_px,
            max_width_px,
            halign,
            ..
        } = params;
        let target = cursor_from_byte(text, byte_offset);
        self.with_buffer(text, params, |buffer| {
            // Iterate visual lines (buffer lines × soft-wrap
            // segments). For each run on the target's buffer line,
            // locate the glyph whose `[start, end)` byte span
            // contains `target.index`. For a trailing-edge caret
            // (no glyph matches in this run), remember the last
            // glyph's `(x + w)` — that's the post-aligned
            // line-end position. Using `run.line_w` instead would
            // ignore cosmic's per-line halign offset and the
            // caret would jump back to the left on right/center-
            // aligned lines. Empty lines (no glyphs) need the
            // explicit halign-aware position because cosmic's
            // per-line offset only kicks in when there's a glyph
            // to offset — `line_w` stays 0.
            let mut last_in_line: Option<(f32, f32, f32)> = None;
            for run in buffer.layout_runs() {
                if run.line_i != target.line {
                    continue;
                }
                let line_end_x = run
                    .glyphs
                    .last()
                    .map(|g| g.x + g.w)
                    .unwrap_or_else(|| empty_line_x(max_width_px, halign));
                last_in_line = Some((line_end_x, run.line_top, run.line_height));
                for g in run.glyphs {
                    if g.start == target.index {
                        return CursorPos {
                            x: g.x,
                            y_top: run.line_top,
                            line_height: run.line_height,
                        };
                    }
                    if g.start < target.index && target.index < g.end {
                        return CursorPos {
                            x: g.x + g.w,
                            y_top: run.line_top,
                            line_height: run.line_height,
                        };
                    }
                }
                // Past the last glyph of this run: continue iterating
                // — a soft-wrap continuation may carry `target.index`.
            }
            let (line_end_x, line_top, line_h) = last_in_line.unwrap_or((0.0, 0.0, line_height_px));
            CursorPos {
                x: line_end_x,
                y_top: line_top,
                line_height: line_h,
            }
        })
        .unwrap_or_else(|| {
            // No shaped buffer (mono fallback OR empty text → cosmic
            // returns INVALID sentinel → `with_buffer` returns None).
            // For empty text the caret must land where cosmic *would*
            // per-line align it; for non-empty mono we walk chars.
            let x = if text.is_empty() {
                empty_line_x(max_width_px, halign)
            } else {
                caret_x_mono_single_line(text, byte_offset, font_size_px)
            };
            CursorPos {
                x,
                y_top: 0.0,
                line_height: line_height_px,
            }
        })
    }

    /// Pixel-position → byte-offset. Multi-line aware on the cosmic
    /// path via `Buffer::hit`. Mono / empty-text falls back to a 1D
    /// `(x ÷ 0.5·font_size)` scan over char boundaries — enough for
    /// headless single-line click tests, ignores `y` entirely.
    pub(crate) fn byte_at_xy(&self, text: &str, x: f32, y: f32, params: ShapeParams) -> usize {
        let font_size_px = params.font_size_px;
        self.with_buffer(text, params, |buffer| {
            buffer
                .hit(x, y)
                .map(|c| cursor_to_byte(text, c))
                .unwrap_or(text.len())
        })
        .unwrap_or_else(|| mono_byte_at_x(text, x, font_size_px))
    }

    /// Append selection rectangles for `range` against the laid-out
    /// `text` to `out` (cleared on entry). One [`Rect`] per visual
    /// line that intersects the range. Multi-line aware via cosmic
    /// `LayoutRun::highlight`; mono / empty-text path emits one rect
    /// spanning the byte range. Caller applies any padding / offset /
    /// scroll math when consuming. Stack-fast for typical line
    /// counts; oversized selections spill to heap (rare, user-driven).
    pub(crate) fn selection_rects(
        &self,
        text: &str,
        range: std::ops::Range<usize>,
        params: ShapeParams,
        out: &mut SelectionRects,
    ) {
        let font_size_px = params.font_size_px;
        let line_height_px = params.line_height_px;
        out.clear();
        if range.is_empty() {
            return;
        }
        let cosmic_ran = self
            .with_buffer(text, params, |buffer| {
                let start = cursor_from_byte(text, range.start);
                let end = cursor_from_byte(text, range.end);
                for run in buffer.layout_runs() {
                    for (x, w) in run.highlight(start, end) {
                        out.push(Rect::new(x, run.line_top, w, run.line_height));
                    }
                }
            })
            .is_some();
        if !cosmic_ran {
            let x0 = caret_x_mono_single_line(text, range.start, font_size_px);
            let x1 = caret_x_mono_single_line(text, range.end, font_size_px);
            out.push(Rect::new(x0, 0.0, x1 - x0, line_height_px));
        }
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

    /// Per-frame maintenance hook. Called once per frame from
    /// `Ui::finalize_frame`, **after** [`Self::sweep_removed`] has pruned
    /// dead `reuse` entries. Currently bounds the reconstructible cosmic
    /// buffer LRU; the home for future per-frame text upkeep. No-op on the
    /// mono fallback.
    pub(crate) fn end_frame(&self) {
        self.inner.borrow_mut().end_frame();
    }

    /// Ensure the shaped buffer referenced by an emitted text run exists.
    /// The retained source text makes any LRU eviction recoverable here.
    pub(crate) fn ensure_buffer(&self, text: &str, key: TextCacheKey) {
        if key.is_invalid() {
            return;
        }
        self.inner
            .borrow_mut()
            .cosmic
            .as_mut()
            .expect("valid TextCacheKey requires a cosmic text shaper")
            .ensure_buffer(text, key);
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
    /// Bound the ordinary content cache. Layout and reuse entries may retain
    /// evicted keys because the encoder reconstructs every emitted run.
    fn end_frame(&mut self) {
        let Some(cosmic) = self.cosmic.as_mut() else {
            return;
        };
        cosmic.end_frame_evict(BUFFER_BUDGET);
    }

    fn dispatch_direct(&mut self, text: &str, params: ShapeParams) -> MeasureResult {
        let ShapeParams {
            font_size_px,
            line_height_px,
            max_width_px,
            ..
        } = params;
        match self.cosmic.as_mut() {
            Some(c) => c.measure(text, params),
            None => mono_measure(
                text,
                font_size_px,
                line_height_px,
                max_width_px,
                LineFit::Wrap,
            ),
        }
    }
}

/// Bypass-cache dispatch: cosmic if installed, mono otherwise. The caller
/// owns reuse accounting, so shaping and map-entry mutation can borrow
/// disjoint `ShaperInner` fields.
fn dispatch(
    cosmic: &mut Option<CosmicMeasure>,
    text: &str,
    text_hash: u64,
    params: ShapeParams,
    fit: LineFit,
    unbounded_key: TextCacheKey,
) -> MeasureResult {
    let ShapeParams {
        font_size_px,
        line_height_px,
        max_width_px,
        ..
    } = params;
    match cosmic.as_mut() {
        // Truncation needs a finite width to cut against; without one
        // it's just an unbounded single line, so fall through to the
        // plain measure.
        Some(c) => match (fit, max_width_px) {
            (LineFit::Ellipsis | LineFit::Clip, Some(_)) => {
                c.measure_truncated(text, params, fit, unbounded_key)
            }
            _ => c.measure_hashed(text, text_hash, params),
        },
        // Mono fallback is single-line; cosmic per-line align
        // can't be applied so `halign` is unused here.
        None => mono_measure(text, font_size_px, line_height_px, max_width_px, fit),
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
#[derive(Clone, Copy, Hash, Eq, PartialEq, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct TextCacheKey {
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
    /// [`FontFamily`] discriminant. Two runs with identical text/size
    /// but different families produce different shaped buffers, so the
    /// key has to discriminate. `u8` because `FontFamily` is `#[repr(u8)]`.
    pub family_q: u8,
    /// [`FontWeight`] discriminant. Two runs with identical text/size/
    /// family but different weight shape against different physical faces
    /// (Regular vs Bold), so the key has to discriminate.
    pub weight_q: u8,
    /// [`HAlign`] discriminant for per-line text alignment. Cosmic
    /// shapes the buffer with line-internal x offsets that depend on
    /// the per-line align, so two runs with identical text/size but
    /// different halign produce different shaped buffers and the key
    /// has to discriminate. `0` (`HAlign::Auto`) means "no per-line
    /// alignment" and matches the previous behaviour.
    pub halign_q: u8,
    /// [`LineFit`] discriminant. Truncating fits bake different source text
    /// into the shaped buffer at the same width, so fit is independent cache
    /// identity rather than part of the text-content hash. This occupies the
    /// former trailing padding byte, keeping the key at 24 bytes.
    pub fit_q: u8,
}

impl TextCacheKey {
    /// Sentinel returned by the mono fallback. Real keys always carry a
    /// nonzero text hash, so that field alone tags validity.
    pub(crate) const INVALID: Self = Self {
        text_hash: 0,
        size_q: 0,
        max_w_q: 0,
        lh_q: 0,
        family_q: 0,
        weight_q: 0,
        halign_q: 0,
        fit_q: 0,
    };

    pub(crate) const fn is_invalid(self) -> bool {
        self.text_hash == 0
    }
}

/// Result of measuring (and, in the cosmic path, shaping) one text run.
#[derive(Clone, Copy, Debug)]
pub struct MeasureResult {
    pub size: Size,
    /// Identifier of the shaped buffer, or [`TextCacheKey::INVALID`] when no
    /// shaping happened (mono fallback). Crate-internal — the renderer's
    /// cache key, not part of the public measurement result.
    pub(crate) key: TextCacheKey,
    /// Width of the widest unbreakable run (typically the longest word).
    /// The wrapping path uses this as the floor when a parent commits a
    /// narrower width: text overflows rather than breaking inside a word.
    /// Equal to `size.w` for the mono fallback (no real word boundaries) and
    /// for single-word inputs.
    pub intrinsic_min: f32,
}

impl MeasureResult {
    /// Zero-size run carrying [`TextCacheKey::INVALID`] — the result for
    /// empty / non-positive-size input on every path. The renderer drops
    /// `INVALID` runs, so nothing paints.
    pub(crate) const INVALID: Self = Self {
        size: Size::ZERO,
        key: TextCacheKey::INVALID,
        intrinsic_min: 0.0,
    };
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
    fit: LineFit,
) -> MeasureResult {
    if text.is_empty() {
        return MeasureResult::INVALID;
    }
    let glyph_w = font_size_px * 0.5;
    let line_h = line_height_px;
    // Mono is a deterministic stub — count one "char" per byte. Correct for
    // ASCII (which is what every test and bench uses); for multibyte input
    // it overcounts, but mono is not a production path.
    let total_chars = text.len() as f32;
    let unbroken_w = total_chars * glyph_w;
    let single_line = matches!(fit, LineFit::Clip | LineFit::Ellipsis);

    let size = match max_width_px {
        None => Size::new(unbroken_w, line_h),
        Some(max) if max >= unbroken_w => Size::new(unbroken_w, line_h),
        // Clip/ellipsis is one line capped at the available width.
        Some(max) if single_line => Size::new(max, line_h),
        Some(max) => {
            let chars_per_line = (max / glyph_w).floor().max(1.0);
            let lines = (total_chars / chars_per_line).ceil().max(1.0);
            Size::new((chars_per_line * glyph_w).min(unbroken_w), lines * line_h)
        }
    };
    // A truncated run shrinks to nothing — zero floor. Otherwise mono has
    // no real word boundaries, so fall back to "the longest run of
    // non-space bytes" as the wrap floor.
    let intrinsic_min = if single_line {
        0.0
    } else {
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
        longest as f32 * glyph_w
    };
    MeasureResult {
        size,
        key: TextCacheKey::INVALID,
        intrinsic_min,
    }
}

/// Caret position returned by [`TextShaper::cursor_xy`]. Top-left in
/// text-local pixels plus the visual line's height (so the renderer
/// can size the caret rect to match the line cosmic-text laid out,
/// not the requested `line_height_px` — they differ when font
/// fallback shifts ascent/descent).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct CursorPos {
    pub(crate) x: f32,
    pub(crate) y_top: f32,
    pub(crate) line_height: f32,
}

/// Caret-x along a single-line mono layout (0.5×font_size per byte).
/// Multi-line aware callers should go through `cursor_xy` instead —
/// this is the cheap path for the mono fallback's degenerate single-
/// line behaviour.
fn caret_x_mono_single_line(text: &str, byte_offset: usize, font_size_px: f32) -> f32 {
    let clamped = byte_offset.min(text.len());
    (clamped as f32) * font_size_px * 0.5
}

/// Where the caret on a zero-glyph line ends up after cosmic's
/// per-line align. Mirrors cosmic's `(line_width - line_w) * factor`
/// formula collapsed for `line_w = 0`. Used by `cursor_xy` when the
/// shaped buffer is missing (empty buffer / mono fallback) — without
/// it, an empty right-aligned multi-line editor would paint its
/// caret at `x = 0` instead of at the right edge.
fn empty_line_x(max_width_px: Option<f32>, halign: HAlign) -> f32 {
    let Some(w) = max_width_px else { return 0.0 };
    match halign {
        HAlign::Center => w * 0.5,
        HAlign::Right => w,
        HAlign::Auto | HAlign::Left | HAlign::Stretch => 0.0,
    }
}

/// Position a measured text block inside `leaf` per `align`: `min`
/// shifted by the alignment offset, `size` = the measured bbox (the
/// composer takes `min` as the glyph origin and `size` as the clip
/// bounds). Glyphs don't stretch, so `Auto`/`Stretch` collapse to
/// start — matches `place_axis` for non-stretchable content — and
/// overflow on an axis clamps that axis's offset to zero so oversized
/// text pins to the leading edge.
///
/// Coordinate-system agnostic: the cascade and encoder pass
/// owner-local / screen-space leaf rects; `TextEdit` passes a
/// zero-origin rect and reads `.min` back as the bare offset for its
/// caret/selection math. One definition for all of them — glyphs,
/// caret, and selection wash must shift by the same offset or the
/// caret drifts off its glyph.
pub(crate) fn text_in_rect(leaf: Rect, measured: Size, align: Align) -> Rect {
    let dx = match align.halign() {
        HAlign::Auto | HAlign::Left | HAlign::Stretch => 0.0,
        HAlign::Center => (leaf.size.w - measured.w) * 0.5,
        HAlign::Right => leaf.size.w - measured.w,
    };
    let dy = match align.valign() {
        VAlign::Auto | VAlign::Top | VAlign::Stretch => 0.0,
        VAlign::Center => (leaf.size.h - measured.h) * 0.5,
        VAlign::Bottom => leaf.size.h - measured.h,
    };
    Rect::new(
        leaf.min.x + dx.max(0.0),
        leaf.min.y + dy.max(0.0),
        measured.w,
        measured.h,
    )
}

/// Inverse of [`caret_x_mono_single_line`]. Picks the char boundary
/// whose prefix-x is closest to `target_x` so click positioning on
/// the mono fallback matches the rendered glyph layout exactly.
fn mono_byte_at_x(text: &str, target_x: f32, font_size_px: f32) -> usize {
    let mut best_off = 0usize;
    let mut best_dist = target_x.abs();
    for (i, ch) in text.char_indices() {
        let next = i + ch.len_utf8();
        let x = caret_x_mono_single_line(text, next, font_size_px);
        let d = (x - target_x).abs();
        if d < best_dist {
            best_dist = d;
            best_off = next;
        }
    }
    best_off
}

/// Map a UTF-8 byte offset into `text` to a cosmic-text `Cursor`:
/// `line` = count of `\n` before the offset, `index` = bytes since
/// the most recent `\n` (or start of text).
fn cursor_from_byte(text: &str, byte_offset: usize) -> cosmic_text::Cursor {
    let mut line = 0usize;
    let mut line_start = 0usize;
    for (i, byte) in text.as_bytes().iter().enumerate() {
        if i >= byte_offset {
            break;
        }
        if *byte == b'\n' {
            line += 1;
            line_start = i + 1;
        }
    }
    cosmic_text::Cursor::new(line, byte_offset.saturating_sub(line_start))
}

/// Inverse of [`cursor_from_byte`]. Walks `text` to find the
/// `line`-th `\n` and adds `cursor.index`.
fn cursor_to_byte(text: &str, cursor: cosmic_text::Cursor) -> usize {
    if cursor.line == 0 {
        return cursor.index.min(text.len());
    }
    let mut line = 0usize;
    for (i, byte) in text.as_bytes().iter().enumerate() {
        if *byte == b'\n' {
            line += 1;
            if line == cursor.line {
                return (i + 1 + cursor.index).min(text.len());
            }
        }
    }
    text.len()
}

/// Cached unbounded shape + most-recent wrap result, validity-checked
/// by authoring `hash`.
#[derive(Clone, Copy, Debug)]
pub(crate) struct TextReuseEntry {
    hash: ContentHash,
    unbounded: MeasureResult,
    wrap: Option<WrapReuse>,
}

/// One cached width-bounded result — the most-recent `target_q` (caller-
/// quantized target), halign, and overflow mode, plus the `MeasureResult`
/// that came out of shaping at that target.
#[derive(Clone, Copy, Debug)]
struct WrapReuse {
    target_q: u32,
    /// Cached halign. Cosmic's per-line align changes glyph positions
    /// inside the shaped buffer, so changing halign invalidates this
    /// slot even when `target_q` is unchanged.
    halign: HAlign,
    /// Overflow mode. A widget that flips mode at the same call site
    /// reshapes — each mode bakes a different buffer (and `Clip`/`Ellipsis`
    /// a different truncated string) at the same target.
    fit: LineFit,
    result: MeasureResult,
}

/// How a width-bounded text run handles overflow. Maps from the public
/// [`crate::shape::TextWrap`] (minus `SingleLine`/`Scroll`, which stay on
/// the unbounded path). Threaded through `shape_wrap` → `dispatch` and
/// folded into the shape cache key.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum LineFit {
    /// Multi-line reflow at the target width.
    Wrap = 0,
    /// One line, hard-cut to the target width with no marker.
    Clip = 1,
    /// One line, cut to the target width with a trailing `…`.
    Ellipsis = 2,
}

#[cfg(any(test, feature = "internals"))]
pub mod test_support {
    #![allow(dead_code)]
    use crate::common::hash::hash_str;
    use crate::shape::TextWrap;
    use crate::text::*;

    impl TextShaper {
        /// Total cache-miss `measure` dispatches.
        pub fn measure_calls(&self) -> u64 {
            self.inner.borrow().measure_calls
        }

        /// `true` iff a reuse entry exists for `(wid, ordinal)`.
        pub fn has_reuse_entry(&self, wid: WidgetId, ordinal: u16) -> bool {
            self.inner.borrow().reuse.contains_key(&(wid, ordinal))
        }

        pub fn has_cosmic_buffer(&self, key: TextCacheKey) -> bool {
            self.inner
                .borrow()
                .cosmic
                .as_ref()
                .is_some_and(|cosmic| cosmic.buffer_for(key).is_some())
        }

        pub fn evict_cosmic_buffers(&self, max_keep: usize) {
            self.inner
                .borrow_mut()
                .cosmic
                .as_mut()
                .expect("cosmic buffer eviction requires a cosmic text shaper")
                .end_frame_evict(max_keep);
        }

        /// Drive the production unbounded-then-truncate sequence at one width.
        pub fn measure_truncated_width_for_bench(
            &self,
            wid: WidgetId,
            text: &str,
            params: ShapeParams,
            wrap: TextWrap,
        ) -> MeasureResult {
            let text_hash = hash_str(text);
            let hash = ContentHash(text_hash);
            self.shape_unbounded(wid, 0, hash, text, text_hash, params);
            let target = params
                .max_width_px
                .expect("truncation benchmark requires a finite width");
            let fit = match wrap {
                TextWrap::Truncate => LineFit::Clip,
                TextWrap::Ellipsis => LineFit::Ellipsis,
                _ => panic!("truncation benchmark requires Truncate or Ellipsis"),
            };
            self.shape_wrap(
                wid,
                0,
                hash,
                text,
                params,
                (target.max(0.0) * 64.0).round() as u32,
                fit,
            )
        }
    }
}

#[cfg(test)]
mod tests;
