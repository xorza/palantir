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

use crate::primitives::{size::Size, widget_id::WidgetId};
use crate::tree::hash::NodeHash;
use rustc_hash::FxHashMap;
use std::cell::RefCell;
use std::collections::hash_map::Entry;
use std::rc::Rc;

pub(crate) mod cosmic;

use crate::text::cosmic::CosmicMeasure;

/// Shared handle to a [`CosmicMeasure`], cloned into both [`TextMeasurer`]
/// (Ui-side measurement) and the renderer's `TextRenderer` (wgpu-side
/// shaping + rasterization). Single-threaded by design (`Rc`); access is
/// sequential — measure during layout, prepare/render during the wgpu
/// frame — so the `RefCell` is just runtime insurance against accidental
/// re-entry.
pub type SharedCosmic = Rc<RefCell<CosmicMeasure>>;

/// Wrap a fresh [`CosmicMeasure`] for sharing between Ui and renderer.
pub fn share(cosmic: CosmicMeasure) -> SharedCosmic {
    Rc::new(RefCell::new(cosmic))
}

/// Stable identifier for a shaped text run, computed at authoring time so
/// `Shape::Text` can carry it through the encoder/composer and the renderer
/// can look up the matching shaped buffer without rehashing.
///
/// Three quantized fields rather than one collapsed `u64` so the renderer
/// can also reuse the size/width components if it wants to (e.g. group runs
/// by size for atlas bin reuse). [`TextCacheKey::INVALID`] is the sentinel
/// returned by the mono fallback — the renderer treats it as "drop this run".
#[repr(C)]
#[derive(Clone, Copy, Hash, Eq, PartialEq, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct TextCacheKey {
    /// 64-bit hash of the source string. `0` for the invalid sentinel.
    pub text_hash: u64,
    /// `font_size_px * 64`, rounded. Quantizing to 1/64 px is below any
    /// visible difference and keeps the key purely integral.
    pub size_q: u32,
    /// `max_width_px * 64`, rounded; `u32::MAX` encodes `None` (unbounded).
    pub max_w_q: u32,
}

impl TextCacheKey {
    /// Sentinel returned by the mono fallback. The renderer skips runs with
    /// this key.
    pub const INVALID: Self = Self {
        text_hash: 0,
        size_q: 0,
        max_w_q: 0,
    };

    pub const fn is_invalid(self) -> bool {
        self.text_hash == 0 && self.size_q == 0 && self.max_w_q == 0
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
fn mono_measure(text: &str, font_size_px: f32, max_width_px: Option<f32>) -> MeasureResult {
    if text.is_empty() {
        return MeasureResult {
            size: Size::ZERO,
            key: TextCacheKey::INVALID,
            intrinsic_min: 0.0,
        };
    }
    let glyph_w = font_size_px * 0.5;
    let line_h = font_size_px;
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

/// Free-function dispatch into the shaper. Takes `&self.cosmic` so
/// the cached entry points can invoke it while holding a
/// `&mut TextReuseEntry` from `reuse` — `&mut self` would over-borrow.
#[inline]
fn dispatch(
    cosmic: &Option<SharedCosmic>,
    text: &str,
    font_size_px: f32,
    max_width_px: Option<f32>,
) -> MeasureResult {
    match cosmic {
        Some(c) => c.borrow_mut().measure(text, font_size_px, max_width_px),
        None => mono_measure(text, font_size_px, max_width_px),
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
pub(crate) struct WrapReuse {
    target_q: u32,
    result: MeasureResult,
}

/// Ui-side measurement façade. Holds an optional shared handle to the real
/// shaper; falls through to [`mono_measure`] when nothing is installed.
/// The renderer holds its own clone of the same handle so layout and
/// rasterization see the same buffer cache.
#[derive(Default)]
pub struct TextMeasurer {
    cosmic: Option<SharedCosmic>,
    /// Total `measure` calls made through this façade. Read by tests
    /// pinning reshape-skip behaviour; cheap enough to leave on in
    /// release.
    pub(crate) measure_calls: u64,
    /// Per-`WidgetId` cross-frame cache of shaping output, lookup-keyed
    /// by identity and validity-checked by authoring hash. Lets layout
    /// skip the `measure` dispatch (and the underlying string-hash +
    /// `RefCell` lock around `CosmicMeasure`) when a Text leaf's
    /// inputs are unchanged across frames. The wrap slot's `target_q`
    /// quantization is layout policy chosen at the call site.
    pub(crate) reuse: FxHashMap<WidgetId, TextReuseEntry>,
}

impl TextMeasurer {
    pub fn new() -> Self {
        Self::default()
    }

    /// Install a shared shaper handle. Pass the same `SharedCosmic` to the
    /// renderer (`WgpuBackend::set_cosmic`) so both sides see one cache.
    ///
    /// Call exactly once, before the first frame. The measure cache and
    /// encode cache key shaping outputs (`measured`, `TextCacheKey`) on
    /// `(WidgetId, subtree_hash, available_q)` — shaper identity is *not*
    /// in either key. Swapping shapers mid-session would let a cache hit
    /// replay a `TextCacheKey` minted by the old shaper against the new
    /// one. If you ever need to support a swap, also invalidate the
    /// measure cache, encode cache, and text reuse map at the swap point.
    pub fn set_cosmic(&mut self, cosmic: SharedCosmic) {
        assert!(
            self.cosmic.is_none(),
            "TextMeasurer::set_cosmic called twice; see doc comment — \
             swapping shapers requires invalidating measure + encode caches"
        );
        self.cosmic = Some(cosmic);
    }

    /// Identity-cached unbounded shape for `wid`, refreshing it (and
    /// clearing any stale wrap entry) when the authoring hash has
    /// shifted. Returns by value because callers typically also call
    /// [`Self::shape_wrap`] on the wrap path, which would borrow-
    /// conflict with a reference into the cache.
    pub(crate) fn shape_unbounded(
        &mut self,
        wid: WidgetId,
        hash: NodeHash,
        text: &str,
        font_size_px: f32,
    ) -> MeasureResult {
        // One hash lookup, all paths: `Entry` gives us a slot that
        // can be read, overwritten, or inserted into without
        // re-hashing. The early-return arm consumes the entry on hit;
        // every other arm falls through to one shared dispatch +
        // write. Disjoint field borrows let `dispatch` run while the
        // slot borrow is held.
        let slot = match self.reuse.entry(wid) {
            Entry::Occupied(o) if o.get().hash == hash => return o.get().unbounded,
            other => other,
        };
        self.measure_calls += 1;
        let unbounded = dispatch(&self.cosmic, text, font_size_px, None);
        let new = TextReuseEntry {
            hash,
            unbounded,
            wrap: None,
        };
        match slot {
            Entry::Occupied(mut o) => *o.get_mut() = new,
            Entry::Vacant(v) => {
                v.insert(new);
            }
        }
        unbounded
    }

    /// Identity-cached wrap shape for `wid` at the caller-quantized
    /// `target_q`. Hits the cache when the same wrap target was used
    /// last frame; otherwise dispatches `measure` and refreshes the
    /// entry. Caller is responsible for having populated the unbounded
    /// entry first via [`Self::shape_unbounded`] — without that, the
    /// wrap result is computed but cannot be cached (no parent entry)
    /// and the next call re-measures.
    pub fn shape_wrap(
        &mut self,
        wid: WidgetId,
        text: &str,
        font_size_px: f32,
        target: f32,
        target_q: u32,
    ) -> MeasureResult {
        if let Some(entry) = self.reuse.get_mut(&wid) {
            if let Some(w) = entry.wrap
                && w.target_q == target_q
            {
                return w.result;
            }
            // Cache miss with existing entry: write back through the same
            // borrow. Disjoint field borrows let `dispatch` run while
            // `entry` is held.
            self.measure_calls += 1;
            let m = dispatch(&self.cosmic, text, font_size_px, Some(target));
            entry.wrap = Some(WrapReuse {
                target_q,
                result: m,
            });
            return m;
        }
        // No prime: dispatch but don't cache.
        self.measure_calls += 1;
        dispatch(&self.cosmic, text, font_size_px, Some(target))
    }

    /// Drop reuse entries for the supplied removed-widget set. Mirrors
    /// the same per-frame diff fed to `Damage::compute` so cleanup
    /// stays bounded under widget churn without a second `seen_ids`
    /// scan.
    pub fn sweep_removed(&mut self, removed: &[WidgetId]) {
        for wid in removed {
            self.reuse.remove(wid);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_is_zero_invalid() {
        let r = mono_measure("", 16.0, None);
        assert_eq!(r.size, Size::ZERO);
        assert!(r.key.is_invalid());
    }

    #[test]
    fn unbroken_matches_legacy_placeholder() {
        // Pre-trait code used `chars * 8.0` × `16.0` at the implicit 16 px size.
        assert_eq!(mono_measure("Hi", 16.0, None).size, Size::new(16.0, 16.0));
        assert_eq!(
            mono_measure("hello!!", 16.0, None).size,
            Size::new(56.0, 16.0)
        );
    }

    #[test]
    fn wraps_when_max_width_below_unbroken() {
        // 8 chars × 8 px = 64 unbroken; max 32 → 4 chars/line, 2 lines.
        let s = mono_measure("12345678", 16.0, Some(32.0)).size;
        assert_eq!(s, Size::new(32.0, 32.0));
    }
}
