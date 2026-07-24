//! Text shaping & measurement.
//!
//! Two paths, one struct each:
//!
//! - [`CosmicMeasure`] — real shaping via `cosmic-text`, with a per-key
//!   shaped-buffer cache. The wgpu backend reuses these `Buffer`s in its
//!   text prepare/append path. Each render run carries its record-local source
//!   span so an encoded-cache miss can restore an evicted shaped buffer.
//! - [`mono_measure`] — deterministic placeholder metric used when no
//!   `CosmicMeasure` is installed. Every glyph is
//!   `font_size_px * 0.5` wide; runs measured this way carry
//!   [`TextShapeKey::INVALID`] and the renderer drops them. Lets the engine
//!   run in tests and headless tools without a font system.
//! - [`TextSystem`] — per-window text coordinator. It owns identity reuse
//!   keyed by widget and within-widget text ordinal while referring to the
//!   app-global [`TextShaper`] shared with the renderer.
//!
//! There's no `TextMeasure` trait: the renderer needs concrete access to
//! `CosmicMeasure`'s `FontSystem` + cache, so a trait would just be a
//! downcast in disguise.
//!
//! [`Ui`]: crate::Ui

use crate::common::hash;
use crate::layout::types::align::{Align, HAlign, VAlign};
use crate::primitives::approx::EPS;
use crate::primitives::num::F32Ext;
use crate::primitives::rect::Rect;
use crate::primitives::size::Size;
use crate::primitives::widget_id::WidgetId;
use crate::text::wrap::TextWrap;
use rustc_hash::{FxHashMap, FxHashSet};
use std::cell::RefCell;
use std::collections::hash_map::Entry;
use std::rc::Rc;
use unicode_segmentation::UnicodeSegmentation;

pub(crate) mod cosmic;
pub(crate) mod wrap;

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
/// [`crate::scene::shapes::record::text_paint_bbox_local`] inflates
/// by this fraction to keep damage covering the worst-case painted
/// extent.
///
/// Single source — `composer::TEXT_SCALE_STEP` re-exports this value.
pub(crate) const TEXT_SCALE_STEP: f32 = 0.005;

use crate::text::cosmic::{CosmicMeasure, RenderSplit};

/// Output buffer for [`TextLayoutProbe::selection_rects`]. Stores selections
/// up to 16 visual lines inline; larger selections retain their spill
/// allocation when the caller reuses the buffer.
pub(crate) const SELECTION_RECTS_INLINE_CAPACITY: usize = 16;
pub(crate) type SelectionRects = tinyvec::TinyVec<[Rect; SELECTION_RECTS_INLINE_CAPACITY]>;

/// Font family picker on [`crate::TextStyle`] and
/// [`crate::Shape::Text`]. `Sans` resolves to bundled Inter (the default
/// proportional face); `Mono` resolves to bundled JetBrains Mono. Both
/// ship inside [`CosmicMeasure::with_bundled_fonts`]; the mono-fallback
/// shaper (when no `CosmicMeasure` is installed) ignores family entirely.
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

/// Font weight picker on [`crate::TextStyle`] and [`crate::Shape::Text`],
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

/// Shared, cloneable text shaper. Holds an optional [`CosmicMeasure`] for
/// real shaping (`None` ⇒ mono fallback) and a `measure_calls` counter for
/// cache-effectiveness tests. Per-window widget identity reuse lives in
/// the crate-internal `TextSystem`.
///
/// Single-threaded by design (`Rc` inside); access is sequential —
/// measurement during layout, prepare/render during the wgpu frame —
/// so the `RefCell` is just runtime insurance against accidental
/// re-entry. Cloning is cheap (refcount bump).
/// `HostShared` retains the canonical handle; its UI and backend capability
/// views give every consumer access to the same
/// content cache.
///
/// Construct with [`Self::with_bundled_fonts`] or [`Self::with_cosmic`].
/// Test and internals builds additionally provide a mono fallback.
#[derive(Clone, Debug)]
pub struct TextShaper {
    /// `pub(crate)` for [`test_support`] observability helpers. Direct
    /// field access from inside the crate is fine; invariants live in
    /// the mutating methods of `TextShaper`, not in encapsulation theater.
    pub(crate) inner: Rc<RefCell<ShaperInner>>,
}

/// Source text paired with its canonical shaping parameters.
#[derive(Clone, Copy, Debug)]
pub(crate) struct TextShapeRequest<'a> {
    pub(crate) text: &'a str,
    pub(crate) key: TextShapeKey,
}

/// Per-window text coordinator. Identity reuse belongs to the window while
/// shaped content buffers and the font system remain shared through
/// [`TextShaper`]. Reuse rows are clock-swept under size pressure.
#[derive(Debug)]
pub(crate) struct TextSystem {
    shaper: TextShaper,
    entries: FxHashMap<(WidgetId, u16), TextReuseEntry>,
    sweep_limit: usize,
}

/// Shaped-buffer measurement plus every layout consequence of one
/// [`TextWrap`] policy.
#[derive(Clone, Copy, Debug)]
pub(crate) struct TextShapeResult {
    pub(crate) measurement: TextMeasurement,
    pub(crate) content_size: Size,
    pub(crate) min_content: Size,
    pub(crate) max_content: Size,
}

impl TextShapeResult {
    const ZERO: Self = Self {
        measurement: TextMeasurement::ZERO,
        content_size: Size::ZERO,
        min_content: Size::ZERO,
        max_content: Size::ZERO,
    };
}

/// One direct text layout exposed for several read-only geometry queries.
#[derive(Debug)]
pub(crate) struct TextLayoutProbe<'a> {
    pub(crate) measurement: TextMeasurement,
    pub(crate) request: TextShapeRequest<'a>,
    buffer: Option<&'a cosmic_text::Buffer>,
}

/// Per-window identity of one text run. The widget and ordinal select its
/// reuse row; [`TextSystem`] derives validity from the shaping inputs.
#[derive(Clone, Copy, Debug)]
pub(crate) struct TextRunIdentity {
    pub(crate) widget_id: WidgetId,
    pub(crate) ordinal: u16,
}

/// Shared mutable state behind the `Rc<RefCell<...>>` in [`TextShaper`].
/// Both [`crate::Ui`] (layout-time measurement) and [`crate::WgpuBackend`]
/// (shaping during render) borrow this; backend only touches `cosmic` via
/// [`TextShaper::with_render_buffers`].
#[derive(Debug)]
pub(crate) struct ShaperInner {
    /// `None` ⇒ mono fallback path. `Some` ⇒ real shaping.
    cosmic: Option<CosmicMeasure>,
    /// Total shaping dispatches: [`TextSystem`] reuse misses plus every
    /// bypass [`TextShaper::with_layout`] call —
    /// which may still hit the cosmic buffer cache, so this counts
    /// dispatches, not reshapes. Identity-cache hits don't increment.
    /// Read by tests pinning reshape-skip behaviour via
    /// [`test_support::measure_calls`].
    pub(crate) measure_calls: u64,
}

/// Max cosmic buffers retained after per-frame maintenance. Backend misses
/// restore entries from retained text sources, so the cache needs no separate
/// live-layout allowance.
const BUFFER_BUDGET: usize = 2048;
const MIN_REUSE_SWEEP_LIMIT: usize = 256;

pub(crate) const TEXT_METRICS_ERROR: &str =
    "font size and line height must be finite and above the UI epsilon";

pub(crate) fn text_metrics_valid(font_size_px: f32, line_height_px: f32) -> bool {
    font_size_px.is_finite()
        && font_size_px > EPS
        && line_height_px.is_finite()
        && line_height_px > EPS
}

impl<'a> TextShapeRequest<'a> {
    pub(crate) fn unbounded(
        text: &'a str,
        font_size_px: f32,
        line_height_px: f32,
        family: FontFamily,
        weight: FontWeight,
    ) -> Option<Self> {
        TextShapeKey::unbounded(
            hash::hash_str(text),
            font_size_px,
            line_height_px,
            family,
            weight,
        )
        .map(|key| Self { text, key })
    }

    pub(crate) fn bounded(self, max_width_px: f32, halign: HAlign, fit: LineFit) -> Option<Self> {
        self.key.bounded(max_width_px, halign, fit).map(|key| Self {
            text: self.text,
            key,
        })
    }

    pub(crate) fn unbounded_version(self) -> Self {
        Self {
            text: self.text,
            key: self.key.unbounded_version(),
        }
    }
}

impl TextShaper {
    /// Real shaping via the supplied [`CosmicMeasure`]. The shaper's
    /// shaped-buffer cache is shared across all clones of this handle.
    pub fn with_cosmic(cosmic: CosmicMeasure) -> Self {
        Self {
            inner: Rc::new(RefCell::new(ShaperInner {
                cosmic: Some(cosmic),
                measure_calls: 0,
            })),
        }
    }

    /// Convenience: cosmic-backed shaper with bundled fonts loaded.
    pub fn with_bundled_fonts() -> Self {
        Self::with_cosmic(CosmicMeasure::with_bundled_fonts())
    }

    /// Shape `text` once and expose its measurement and geometry for the
    /// duration of `body`.
    pub(crate) fn with_layout<R>(
        &self,
        request: TextShapeRequest<'_>,
        body: impl FnOnce(TextLayoutProbe<'_>) -> R,
    ) -> R {
        if request.text.is_empty() {
            return body(TextLayoutProbe {
                measurement: TextMeasurement::ZERO,
                request,
                buffer: None,
            });
        }

        let mut inner = self.inner.borrow_mut();
        inner.measure_calls += 1;
        let measurement = inner.dispatch_direct(request);
        let buffer = inner
            .cosmic
            .as_ref()
            .and_then(|cosmic| cosmic.buffer_for(measurement.key));
        body(TextLayoutProbe {
            measurement,
            request,
            buffer,
        })
    }
}

impl TextSystem {
    pub(crate) fn new(shaper: TextShaper) -> Self {
        Self {
            shaper,
            entries: FxHashMap::default(),
            sweep_limit: MIN_REUSE_SWEEP_LIMIT,
        }
    }

    pub(crate) fn end_frame(&mut self, removed: &FxHashSet<WidgetId>) {
        self.shaper.end_frame();
        let previous_len = self.entries.len();
        if previous_len > self.sweep_limit {
            if removed.is_empty() {
                self.entries
                    .retain(|_, entry| std::mem::take(&mut entry.hot));
            } else {
                self.entries.retain(|(widget_id, _), entry| {
                    !removed.contains(widget_id) && std::mem::take(&mut entry.hot)
                });
            }
            self.sweep_limit = next_reuse_sweep_limit(self.entries.len());
            return;
        }
        if removed.is_empty() {
            return;
        }
        self.entries
            .retain(|(widget_id, _), _| !removed.contains(widget_id));
        if self.entries.len() != previous_len {
            self.sweep_limit = next_reuse_sweep_limit(self.entries.len());
        }
    }

    /// Shape one identity-cached text run. The unbounded measurement remains
    /// the reuse root; bounded policies derive their target from it and cache
    /// the most recent resolved measurement in the same operation. Inlining
    /// lets each hot caller erase the result fields it does not consume.
    #[inline]
    pub(crate) fn shape(
        &mut self,
        identity: TextRunIdentity,
        request: TextShapeRequest<'_>,
        wrap_policy: TextWrap,
        halign: HAlign,
        available_width_px: Option<f32>,
    ) -> TextShapeResult {
        let shaper = &self.shaper;
        let request = request.unbounded_version();
        if request.text.is_empty() {
            return TextShapeResult::ZERO;
        }

        let refresh = || {
            let mut inner = shaper.inner.borrow_mut();
            inner.measure_calls += 1;
            let unbounded = dispatch(&mut inner.cosmic, request);
            TextReuseEntry {
                key: request.key,
                unbounded,
                wrap: None,
                hot: true,
            }
        };
        let entry = match self.entries.entry((identity.widget_id, identity.ordinal)) {
            Entry::Occupied(mut occupied) => {
                if occupied.get().key != request.key {
                    occupied.insert(refresh());
                } else {
                    occupied.get_mut().hot = true;
                }
                occupied.into_mut()
            }
            Entry::Vacant(vacant) => vacant.insert(refresh()),
        };
        if let Some(width) = available_width_px {
            debug_assert!(width.is_finite());
        }
        let unbounded = entry.unbounded;
        let zero_width = Size::new(0.0, unbounded.size.h);
        match wrap_policy {
            TextWrap::SingleLine => TextShapeResult {
                measurement: unbounded,
                content_size: unbounded.size,
                min_content: unbounded.size,
                max_content: unbounded.size,
            },
            // Scroll owns clipping and panning, so its full run creates no width demand.
            TextWrap::Scroll => TextShapeResult {
                measurement: unbounded,
                content_size: zero_width,
                min_content: zero_width,
                max_content: zero_width,
            },
            TextWrap::Truncate => {
                let measurement = available_width_px.map_or(unbounded, |width| {
                    resolve_bounded_measurement(
                        shaper,
                        entry,
                        request,
                        width,
                        halign,
                        LineFit::Clip,
                    )
                });
                TextShapeResult {
                    measurement,
                    content_size: measurement.size,
                    min_content: zero_width,
                    max_content: unbounded.size,
                }
            }
            TextWrap::Ellipsis => {
                let measurement = available_width_px.map_or(unbounded, |width| {
                    resolve_bounded_measurement(
                        shaper,
                        entry,
                        request,
                        width,
                        halign,
                        LineFit::Ellipsis,
                    )
                });
                TextShapeResult {
                    measurement,
                    content_size: measurement.size,
                    min_content: zero_width,
                    max_content: unbounded.size,
                }
            }
            TextWrap::Wrap => {
                let measurement = available_width_px.map_or(unbounded, |width| {
                    resolve_bounded_measurement(
                        shaper,
                        entry,
                        request,
                        width,
                        halign,
                        LineFit::Wrap,
                    )
                });
                TextShapeResult {
                    measurement,
                    content_size: measurement.size,
                    min_content: zero_width,
                    max_content: unbounded.size,
                }
            }
            TextWrap::WrapWithOverflow => {
                let measurement = available_width_px.map_or(unbounded, |width| {
                    resolve_bounded_measurement(
                        shaper,
                        entry,
                        request,
                        width.max(unbounded.intrinsic_min),
                        halign,
                        LineFit::Wrap,
                    )
                });
                TextShapeResult {
                    measurement,
                    content_size: measurement.size,
                    min_content: Size::new(unbounded.intrinsic_min, unbounded.size.h),
                    max_content: unbounded.size,
                }
            }
        }
    }
}

fn resolve_bounded_measurement(
    shaper: &TextShaper,
    entry: &mut TextReuseEntry,
    request: TextShapeRequest<'_>,
    target_width_px: f32,
    halign: HAlign,
    fit: LineFit,
) -> TextMeasurement {
    let target_width_px = wrap::canonical_wrap_width(target_width_px);
    let request = request
        .bounded(target_width_px, halign, fit)
        .expect("canonical text wrap width must be valid");
    if let Some(wrap) = entry.wrap
        && wrap.key == request.key
    {
        return wrap.result;
    }
    let mut inner = shaper.inner.borrow_mut();
    let ShaperInner {
        cosmic,
        measure_calls,
        ..
    } = &mut *inner;
    *measure_calls += 1;
    let measurement = dispatch(cosmic, request);
    entry.wrap = Some(WrapReuse {
        key: request.key,
        result: measurement,
    });
    measurement
}

impl TextShaper {
    /// (x, y_top, line_height) for the caret at `byte_offset` inside
    /// `text` rendered at `(font_size_px, line_height_px)` with an
    /// optional wrap `max_width_px`. Multi-line aware via cosmic-text
    /// layout runs (each `\n` and each soft-wrap segment becomes a
    /// distinct visual line). Mono fallback / empty-text path
    /// collapses to a 1D layout — `y_top = 0`, `x` from a flat mono
    /// per-byte estimate — usable for tests / headless.
    pub(crate) fn cursor_position(
        &self,
        request: TextShapeRequest<'_>,
        byte_offset: usize,
    ) -> CursorPos {
        self.with_layout(request, |layout| layout.cursor_xy(byte_offset))
    }

    /// Pixel-position → byte-offset. Multi-line aware on the cosmic
    /// path via `Buffer::hit`. Mono / empty-text falls back to a 1D
    /// `(x ÷ 0.5·font_size)` scan over char boundaries — enough for
    /// headless single-line click tests, ignores `y` entirely.
    pub(crate) fn hit_test(&self, request: TextShapeRequest<'_>, x: f32, y: f32) -> usize {
        self.with_layout(request, |layout| layout.byte_at_xy(x, y))
    }

    /// Bounds the reconstructible cosmic buffer LRU. Called by
    /// [`TextSystem::end_frame`]; no-op on the mono fallback.
    pub(crate) fn end_frame(&self) {
        self.inner.borrow_mut().end_frame();
    }

    /// Restore every requested buffer, then lend the font system and buffer
    /// lookup to the renderer under the same exclusive shaper borrow.
    pub(crate) fn with_render_buffers<'a, R>(
        &self,
        requests: impl IntoIterator<Item = TextShapeRequest<'a>>,
        body: impl FnOnce(RenderSplit<'_>) -> R,
    ) -> R {
        let mut inner = self.inner.borrow_mut();
        let cosmic = inner
            .cosmic
            .as_mut()
            .expect("valid text render requests require a cosmic text shaper");
        for request in requests {
            debug_assert!(!request.key.is_invalid());
            debug_assert_eq!(hash::hash_str(request.text), request.key.text_hash);
            cosmic.ensure_buffer(request);
        }
        body(cosmic.split_for_render())
    }
}

impl TextLayoutProbe<'_> {
    pub(crate) fn cursor_xy(&self, byte_offset: usize) -> CursorPos {
        let font_size_px = self.request.key.font_size_px();
        let line_height_px = self.request.key.line_height_px();
        let max_width_px = self.request.key.max_width_px();
        let halign = self.request.key.halign();
        let target = cursor_from_byte(self.request.text, byte_offset);
        let Some(buffer) = self.buffer else {
            let x = if self.request.text.is_empty() {
                empty_line_x(max_width_px, halign)
            } else {
                caret_x_mono_single_line(self.request.text, byte_offset, font_size_px)
            };
            return CursorPos {
                x,
                y_top: 0.0,
                line_height: line_height_px,
            };
        };

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
            for glyph in run.glyphs {
                if glyph.start == target.index {
                    return CursorPos {
                        x: glyph.x,
                        y_top: run.line_top,
                        line_height: run.line_height,
                    };
                }
                if glyph.start < target.index && target.index < glyph.end {
                    return CursorPos {
                        x: glyph.x + glyph.w,
                        y_top: run.line_top,
                        line_height: run.line_height,
                    };
                }
            }
        }
        let (line_end_x, line_top, line_height) =
            last_in_line.unwrap_or((0.0, 0.0, line_height_px));
        CursorPos {
            x: line_end_x,
            y_top: line_top,
            line_height,
        }
    }

    fn byte_at_xy(&self, x: f32, y: f32) -> usize {
        match self.buffer {
            Some(buffer) => buffer
                .hit(x, y)
                .map(|cursor| cursor_to_byte(self.request.text, cursor))
                .unwrap_or(self.request.text.len()),
            None => mono_byte_at_x(self.request.text, x, self.request.key.font_size_px()),
        }
    }

    pub(crate) fn selection_rects(&self, range: std::ops::Range<usize>, out: &mut SelectionRects) {
        out.clear();
        if range.is_empty() {
            return;
        }
        let Some(buffer) = self.buffer else {
            let font_size_px = self.request.key.font_size_px();
            let x0 = caret_x_mono_single_line(self.request.text, range.start, font_size_px);
            let x1 = caret_x_mono_single_line(self.request.text, range.end, font_size_px);
            out.push(Rect::new(
                x0,
                0.0,
                x1 - x0,
                self.request.key.line_height_px(),
            ));
            return;
        };
        let start = cursor_from_byte(self.request.text, range.start);
        let end = cursor_from_byte(self.request.text, range.end);
        for run in buffer.layout_runs() {
            push_run_selection_rects(&run, start, end, out);
        }
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

    fn dispatch_direct(&mut self, request: TextShapeRequest<'_>) -> TextMeasurement {
        match self.cosmic.as_mut() {
            Some(cosmic) => cosmic.shape(request),
            None => mono_measure(request),
        }
    }
}

/// Bypass-cache dispatch: cosmic if installed, mono otherwise. The caller
/// owns reuse accounting, so shaping and map-entry mutation can borrow
/// disjoint `ShaperInner` fields.
fn dispatch(cosmic: &mut Option<CosmicMeasure>, request: TextShapeRequest<'_>) -> TextMeasurement {
    match cosmic.as_mut() {
        Some(cosmic) => cosmic.shape(request),
        None => mono_measure(request),
    }
}

/// Canonical shaping parameters and stable shaped-buffer identity. Layout
/// derives it from `ShapeRecord::Text`; the encoder carries it through the
/// composer so the renderer can restore the matching buffer without rehashing
/// or reconstructing a second parameter representation.
///
/// Three quantized fields rather than one collapsed `u64` so the renderer
/// can also reuse the size/width components if it wants to (e.g. group runs
/// by size for atlas bin reuse). [`TextShapeKey::INVALID`] is the sentinel
/// returned by the mono fallback — the renderer treats it as "drop this run".
#[repr(C)]
#[derive(Clone, Copy, Hash, Eq, PartialEq, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct TextShapeKey {
    /// 64-bit hash of the source string. `0` for the invalid sentinel.
    pub(crate) text_hash: u64,
    /// `font_size_px * 64`, rounded. Quantizing to 1/64 px is below any
    /// visible difference and keeps the key purely integral.
    pub(crate) size_q: u32,
    /// `max_width_px * 64`, rounded; `u32::MAX` encodes `None` (unbounded).
    pub(crate) max_w_q: u32,
    /// `line_height_px * 64`, rounded. Two `ShapeRecord::Text` runs at the
    /// same font-size but different leading produce different shaped
    /// buffers (different `Metrics::new`), so the key has to discriminate.
    pub(crate) lh_q: u32,
    /// [`FontFamily`] discriminant. Two runs with identical text/size
    /// but different families produce different shaped buffers, so the
    /// key has to discriminate. `u8` because `FontFamily` is `#[repr(u8)]`.
    pub(crate) family_q: u8,
    /// [`FontWeight`] discriminant. Two runs with identical text/size/
    /// family but different weight shape against different physical faces
    /// (Regular vs Bold), so the key has to discriminate.
    pub(crate) weight_q: u8,
    /// [`HAlign`] discriminant for per-line text alignment. Cosmic
    /// shapes the buffer with line-internal x offsets that depend on
    /// the per-line align, so two runs with identical text/size but
    /// different halign produce different shaped buffers and the key
    /// has to discriminate. `0` (`HAlign::Auto`) means "no per-line
    /// alignment" and matches the previous behaviour.
    pub(crate) halign_q: u8,
    /// [`LineFit`] discriminant. Truncating fits bake different source text
    /// into the shaped buffer at the same width, so fit is independent cache
    /// identity rather than part of the text-content hash. This occupies the
    /// former trailing padding byte, keeping the key at 24 bytes.
    pub(crate) fit_q: u8,
}

const MAX_W_NONE: u32 = u32::MAX;

impl TextShapeKey {
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

    fn unbounded(
        text_hash: u64,
        font_size_px: f32,
        line_height_px: f32,
        family: FontFamily,
        weight: FontWeight,
    ) -> Option<Self> {
        if !text_metrics_valid(font_size_px, line_height_px) {
            return None;
        }
        Some(Self {
            text_hash: text_hash.max(1),
            size_q: quantize_metric(font_size_px),
            max_w_q: MAX_W_NONE,
            lh_q: quantize_metric(line_height_px),
            family_q: family as u8,
            weight_q: weight as u8,
            halign_q: HAlign::Auto as u8,
            fit_q: LineFit::Wrap as u8,
        })
    }

    fn bounded(self, max_width_px: f32, halign: HAlign, fit: LineFit) -> Option<Self> {
        if !max_width_px.is_finite() || max_width_px < 0.0 {
            return None;
        }
        Some(Self {
            max_w_q: quantize_width(max_width_px).min(MAX_W_NONE - 1),
            halign_q: match fit {
                LineFit::Wrap => halign as u8,
                LineFit::Clip | LineFit::Ellipsis => HAlign::Auto as u8,
            },
            fit_q: fit as u8,
            ..self
        })
    }

    pub(crate) fn unbounded_version(self) -> Self {
        Self {
            max_w_q: MAX_W_NONE,
            halign_q: HAlign::Auto as u8,
            fit_q: LineFit::Wrap as u8,
            ..self
        }
    }

    pub(crate) fn font_size_px(self) -> f32 {
        dequantize(self.size_q)
    }

    pub(crate) fn line_height_px(self) -> f32 {
        dequantize(self.lh_q)
    }

    pub(crate) fn max_width_px(self) -> Option<f32> {
        (self.max_w_q != MAX_W_NONE).then(|| dequantize(self.max_w_q))
    }

    pub(crate) fn family(self) -> FontFamily {
        match self.family_q {
            0 => FontFamily::Sans,
            1 => FontFamily::Mono,
            other => panic!("invalid FontFamily discriminant in TextShapeKey: {other}"),
        }
    }

    pub(crate) fn weight(self) -> FontWeight {
        match self.weight_q {
            0 => FontWeight::Regular,
            1 => FontWeight::Bold,
            other => panic!("invalid FontWeight discriminant in TextShapeKey: {other}"),
        }
    }

    pub(crate) fn halign(self) -> HAlign {
        match self.halign_q {
            0 => HAlign::Auto,
            1 => HAlign::Left,
            2 => HAlign::Center,
            3 => HAlign::Right,
            4 => HAlign::Stretch,
            other => panic!("invalid HAlign discriminant in TextShapeKey: {other}"),
        }
    }

    pub(crate) fn fit(self) -> LineFit {
        match self.fit_q {
            0 => LineFit::Wrap,
            1 => LineFit::Clip,
            2 => LineFit::Ellipsis,
            other => panic!("invalid LineFit discriminant in TextShapeKey: {other}"),
        }
    }
}

fn quantize_width(value: f32) -> u32 {
    (value.max(0.0) * 64.0).fast_round() as u32
}

fn quantize_metric(value: f32) -> u32 {
    quantize_width(value).max(1)
}

fn dequantize(value: u32) -> f32 {
    value as f32 / 64.0
}

/// Measurement of one text run, including its intrinsic wrapping floor.
#[derive(Clone, Copy, Debug)]
pub(crate) struct TextMeasurement {
    pub(crate) size: Size,
    /// Identifier of the shaped buffer, or [`TextShapeKey::INVALID`] when no
    /// shaping happened (mono fallback).
    pub(crate) key: TextShapeKey,
    /// Width of the widest unbreakable run (typically the longest word).
    /// The wrapping path uses this as the floor when a parent commits a
    /// narrower width: text overflows rather than breaking inside a word.
    /// Equal to `size.w` for the mono fallback (no real word boundaries) and
    /// for single-word inputs.
    pub(crate) intrinsic_min: f32,
}

impl TextMeasurement {
    /// Successful empty-text measurement. It has no shaped buffer for the
    /// renderer to resolve.
    pub(crate) const ZERO: Self = Self {
        size: Size::ZERO,
        key: TextShapeKey::INVALID,
        intrinsic_min: 0.0,
    };
}

/// Deterministic placeholder metric used when [`crate::Ui`] has no
/// [`CosmicMeasure`] installed. Every glyph is `font_size_px * 0.5` wide and
/// the line uses `line_height_px`; wrapping is approximated by simple
/// character-count division. At the historical 16 px font size this is the
/// 8 px/char × 16 px line layout the engine was hard-coded to before text
/// shaping landed, which is what existing layout tests pin.
///
/// Always returns [`TextShapeKey::INVALID`] — there's no shaped buffer to
/// look up, so the renderer drops these runs cleanly.
fn mono_measure(request: TextShapeRequest<'_>) -> TextMeasurement {
    let text = request.text;
    if text.is_empty() {
        return TextMeasurement::ZERO;
    }
    let font_size_px = request.key.font_size_px();
    let line_height_px = request.key.line_height_px();
    let max_width_px = request.key.max_width_px();
    let fit = request.key.fit();
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
    TextMeasurement {
        size,
        key: TextShapeKey::INVALID,
        intrinsic_min,
    }
}

/// Caret position returned by [`TextShaper::cursor_position`]. Top-left in
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
/// start — matches arrange-axis placement for non-stretchable content — and
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

// `LayoutRun::highlight` builds a temporary `Vec` per run, so stream its spans directly.
fn push_run_selection_rects(
    run: &cosmic_text::LayoutRun<'_>,
    cursor_start: cosmic_text::Cursor,
    cursor_end: cosmic_text::Cursor,
    out: &mut SelectionRects,
) {
    let mut selected: Option<(f32, f32)> = None;
    let mut flush = |selected: &mut Option<(f32, f32)>| {
        if let Some((min_x, max_x)) = selected.take() {
            let width = max_x - min_x;
            if width > 0.0 {
                out.push(Rect::new(min_x, run.line_top, width, run.line_height));
            }
        }
    };

    for glyph in run.glyphs {
        let cluster = &run.text[glyph.start..glyph.end];
        let total = cluster.grapheme_indices(true).count().max(1);
        let grapheme_width = glyph.w / total as f32;
        let mut x = glyph.x;
        for (i, grapheme) in cluster.grapheme_indices(true) {
            let start = glyph.start + i;
            let end = start + grapheme.len();
            let is_selected = (cursor_start.line != run.line_i || end > cursor_start.index)
                && (cursor_end.line != run.line_i || start < cursor_end.index);
            if is_selected {
                selected = Some(match selected {
                    Some((min, max)) => (min.min(x), max.max(x + grapheme_width)),
                    None => (x, x + grapheme_width),
                });
            } else {
                flush(&mut selected);
            }
            x += grapheme_width;
        }
    }
    flush(&mut selected);
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

/// Cached unbounded shape + most-recent wrap result.
#[derive(Clone, Copy, Debug)]
struct TextReuseEntry {
    key: TextShapeKey,
    unbounded: TextMeasurement,
    wrap: Option<WrapReuse>,
    hot: bool,
}

fn next_reuse_sweep_limit(len: usize) -> usize {
    len.saturating_add(1)
        .checked_next_power_of_two()
        .unwrap_or(usize::MAX)
        .max(MIN_REUSE_SWEEP_LIMIT)
}

/// One cached width-bounded result.
#[derive(Clone, Copy, Debug)]
struct WrapReuse {
    key: TextShapeKey,
    result: TextMeasurement,
}

/// How a width-bounded text run handles overflow. Maps from the public
/// [`crate::TextWrap`] (minus `SingleLine`/`Scroll`, which stay on
/// the unbounded path). Resolved by [`TextSystem::shape`] and folded into
/// the shape cache key.
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
pub(crate) mod test_support {
    #![allow(dead_code)]
    use crate::text::*;

    #[derive(Clone, Copy, Debug)]
    pub(crate) struct TestShape {
        pub(crate) font_size_px: f32,
        pub(crate) line_height_px: f32,
        pub(crate) max_width_px: Option<f32>,
        pub(crate) family: FontFamily,
        pub(crate) weight: FontWeight,
        pub(crate) halign: HAlign,
    }

    impl TestShape {
        fn unbounded_request<'a>(self, text: &'a str) -> Option<TextShapeRequest<'a>> {
            TextShapeRequest::unbounded(
                text,
                self.font_size_px,
                self.line_height_px,
                self.family,
                self.weight,
            )
        }

        fn request<'a>(self, text: &'a str, fit: LineFit) -> Option<TextShapeRequest<'a>> {
            let request = self.unbounded_request(text)?;
            match self.max_width_px {
                Some(width) => request.bounded(width, self.halign, fit),
                None => Some(request),
            }
        }
    }

    pub(crate) trait CosmicMeasureTestExt {
        fn measure(&mut self, text: &str, shape: TestShape) -> TextMeasurement;

        fn measure_truncated(
            &mut self,
            text: &str,
            shape: TestShape,
            fit: LineFit,
            unbounded_key: TextShapeKey,
        ) -> TextMeasurement;
    }

    impl CosmicMeasureTestExt for CosmicMeasure {
        fn measure(&mut self, text: &str, shape: TestShape) -> TextMeasurement {
            CosmicMeasure::shape(self, shape.request(text, LineFit::Wrap).unwrap())
        }

        fn measure_truncated(
            &mut self,
            text: &str,
            shape: TestShape,
            fit: LineFit,
            unbounded_key: TextShapeKey,
        ) -> TextMeasurement {
            let request = shape.request(text, fit).unwrap();
            debug_assert_eq!(request.key.unbounded_version(), unbounded_key);
            CosmicMeasure::shape(self, request)
        }
    }

    pub(crate) trait TextShaperTestExt {
        fn measure(&self, text: &str, shape: TestShape) -> Option<TextMeasurement>;

        fn probe_layout<R>(
            &self,
            text: &str,
            shape: TestShape,
            body: impl FnOnce(TextLayoutProbe<'_>) -> R,
        ) -> Option<R>;

        fn cursor_xy(&self, text: &str, byte_offset: usize, shape: TestShape) -> CursorPos;

        fn byte_at_xy(&self, text: &str, x: f32, y: f32, shape: TestShape) -> usize;
    }

    impl TextShaperTestExt for TextShaper {
        fn measure(&self, text: &str, shape: TestShape) -> Option<TextMeasurement> {
            shape
                .request(text, LineFit::Wrap)
                .map(|request| TextShaper::with_layout(self, request, |probe| probe.measurement))
        }

        fn probe_layout<R>(
            &self,
            text: &str,
            shape: TestShape,
            body: impl FnOnce(TextLayoutProbe<'_>) -> R,
        ) -> Option<R> {
            shape
                .request(text, LineFit::Wrap)
                .map(|request| TextShaper::with_layout(self, request, body))
        }

        fn cursor_xy(&self, text: &str, byte_offset: usize, shape: TestShape) -> CursorPos {
            TextShaper::cursor_position(
                self,
                shape.request(text, LineFit::Wrap).unwrap(),
                byte_offset,
            )
        }

        fn byte_at_xy(&self, text: &str, x: f32, y: f32, shape: TestShape) -> usize {
            TextShaper::hit_test(self, shape.request(text, LineFit::Wrap).unwrap(), x, y)
        }
    }

    pub(crate) trait TextSystemTestExt {
        fn shape_run(
            &mut self,
            identity: TextRunIdentity,
            text: &str,
            shape: TestShape,
            wrap_policy: TextWrap,
        ) -> Option<TextMeasurement>;
    }

    impl TextSystemTestExt for TextSystem {
        fn shape_run(
            &mut self,
            identity: TextRunIdentity,
            text: &str,
            shape: TestShape,
            wrap_policy: TextWrap,
        ) -> Option<TextMeasurement> {
            shape.unbounded_request(text).map(|request| {
                TextSystem::shape(
                    self,
                    identity,
                    request,
                    wrap_policy,
                    shape.halign,
                    shape.max_width_px,
                )
                .measurement
            })
        }
    }

    impl Default for TextShaper {
        fn default() -> Self {
            Self::mono()
        }
    }

    impl Default for TextSystem {
        fn default() -> Self {
            Self::new(TextShaper::default())
        }
    }

    impl TextShaper {
        pub fn mono() -> Self {
            Self {
                inner: Rc::new(RefCell::new(ShaperInner {
                    cosmic: None,
                    measure_calls: 0,
                })),
            }
        }

        /// Total cache-miss `measure` dispatches.
        pub(crate) fn measure_calls(&self) -> u64 {
            self.inner.borrow().measure_calls
        }

        pub(crate) fn has_cosmic_buffer(&self, key: TextShapeKey) -> bool {
            self.inner
                .borrow()
                .cosmic
                .as_ref()
                .is_some_and(|cosmic| cosmic.buffer_for(key).is_some())
        }

        pub(crate) fn evict_cosmic_buffers(&self, max_keep: usize) {
            self.inner
                .borrow_mut()
                .cosmic
                .as_mut()
                .expect("cosmic buffer eviction requires a cosmic text shaper")
                .end_frame_evict(max_keep);
        }
    }

    impl TextSystem {
        /// `true` iff an identity row exists for `(wid, ordinal)`.
        pub(crate) fn has_entry(&self, wid: WidgetId, ordinal: u16) -> bool {
            self.entries.contains_key(&(wid, ordinal))
        }
    }
}

#[cfg(test)]
mod tests;
