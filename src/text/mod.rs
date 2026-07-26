//! Text shaping & measurement.
//!
//! Two paths, one struct each:
//!
//! - [`CosmicMeasure`] — real shaping via `cosmic-text`, with a per-key
//!   shaped-buffer cache. The wgpu backend replays these buffers through
//!   [`render`]. Each render run carries its record-local
//!   source span so an encoded-cache miss can restore an evicted shaped
//!   buffer. The only production backend.
//! - [`mono::internals::measure`] — deterministic placeholder metric behind
//!   the test/internals-only `TextShaper::test_mono`. Every glyph is
//!   `font_size_px * 0.5` wide; it mints no shaped buffer, so `TextSystem`
//!   reports [`TextShapeKey::INVALID`] for those runs and the renderer drops
//!   them. Lets the engine run in tests and headless tools without a font
//!   system.
//! - [`TextSystem`](crate::text::system::TextSystem) — per-window text
//!   coordinator. It owns reuse slots keyed by widget and within-widget
//!   text ordinal while referring to the app-global [`TextShaper`] shared
//!   with the renderer.
//!
//! There's no `TextMeasure` trait: the render path needs `CosmicMeasure`'s
//! shaped buffers + font system (leased cosmic-free through
//! [`render`]), which the mono fallback cannot provide,
//! so a trait would just be a downcast in disguise.
//!
//! Module layout: this file owns the shared vocabulary ([`FontFamily`],
//! [`FontWeight`], [`TextShapeRequest`], [`TextRoot`]) and the
//! [`TextShaper`] coordinator; [`key`] the quantized cache identity;
//! [`system`] the per-window reuse slots; [`probe`] read-only
//! caret / hit-test / selection geometry; [`render`] the cosmic-free
//! [`cosmic`] real shaping; [`wrap`] the wrap-policy vocabulary.

use crate::common::hash;
use crate::layout::types::align::HAlign;
use crate::primitives::size::Size;
use crate::text::cosmic::CosmicMeasure;
use crate::text::key::TextShapeKey;
use crate::text::probe::TextLayoutProbe;
use crate::text::render::TextRenderSession;
use crate::text::wrap::LineFit;
use std::cell::{RefCell, RefMut};
use std::rc::Rc;

#[cfg(feature = "internals")]
pub(crate) mod bench;
mod cosmic;
pub(crate) mod key;
mod mono;
pub(crate) mod probe;
pub(crate) mod render;
pub(crate) mod system;
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

/// Font family picker on [`crate::TextStyle`] and
/// [`crate::Shape::Text`]. `Sans` resolves to bundled Inter (the default
/// proportional face); `Mono` resolves to bundled JetBrains Mono. Both
/// ship inside `CosmicMeasure::with_bundled_fonts`; the test-only mono
/// fallback ignores family entirely.
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

/// Shared, cloneable text shaper. Holds the `CosmicMeasure` used for all
/// real shaping plus a test/internals-only `measure_calls` counter for
/// cache-effectiveness tests. Per-window reuse slots live in the
/// crate-internal `TextSystem`.
///
/// Single-threaded by design (`Rc` inside); access is sequential —
/// measurement during layout, prepare/render during the wgpu frame —
/// so the `RefCell` is just runtime insurance against accidental
/// re-entry. Cloning is cheap (refcount bump).
/// `HostShared` retains the canonical handle; its UI and backend capability
/// views give every consumer access to the same
/// content cache.
///
/// Construct with [`Self::new`] / `Default` (bundled fonts). Test and
/// internals builds additionally provide the mono fallback
/// `Self::test_mono`.
#[derive(Clone, Debug)]
pub struct TextShaper {
    inner: Rc<RefCell<ShaperInner>>,
}

/// Source text paired with its canonical shaping parameters.
#[derive(Clone, Copy, Debug)]
pub(crate) struct TextShapeRequest<'a> {
    pub(crate) text: &'a str,
    pub(crate) key: TextShapeKey,
}

/// Shared mutable state behind the `Rc<RefCell<...>>` in [`TextShaper`].
/// Both [`crate::Ui`] (layout-time measurement) and [`crate::WgpuBackend`]
/// (shaping during render) borrow this; backend only touches `cosmic` via
/// [`TextShaper::render_session`].
#[derive(Debug)]
pub(crate) struct ShaperInner {
    /// `None` ⇒ the test/internals-only mono fallback. `TextShaper::new`
    /// always installs `Some`, and `TextShaper::test_mono` is the only
    /// `None` construction, so production never observes it.
    cosmic: Option<CosmicMeasure>,
    /// Total [`ShaperInner::dispatch`] calls: `TextSystem` reuse misses
    /// plus every bypass [`TextShaper::layout`] call —
    /// which may still hit the cosmic buffer cache, so this counts
    /// dispatches, not reshapes. Reuse-slot hits don't increment.
    /// Read by tests pinning reshape-skip behaviour via
    /// [`internals::measure_calls`]; production builds carry neither
    /// the field nor the write.
    #[cfg(any(test, feature = "internals"))]
    measure_calls: u64,
}

impl ShaperInner {
    fn new(cosmic: Option<CosmicMeasure>) -> Self {
        Self {
            cosmic,
            #[cfg(any(test, feature = "internals"))]
            measure_calls: 0,
        }
    }
}

/// Max cosmic buffers retained after per-frame maintenance. Backend misses
/// restore entries from retained text sources, so the cache needs no separate
/// live-layout allowance.
const BUFFER_BUDGET: usize = 2048;

impl<'a> TextShapeRequest<'a> {
    /// Metrics were validated at record time; invalid values here are a
    /// logic error, debug-asserted by [`TextShapeKey::unbounded`].
    pub(crate) fn unbounded(
        text: &'a str,
        font_size_px: f32,
        line_height_px: f32,
        family: FontFamily,
        weight: FontWeight,
    ) -> Self {
        Self {
            text,
            key: TextShapeKey::unbounded(
                hash::hash_str(text),
                font_size_px,
                line_height_px,
                family,
                weight,
            ),
        }
    }

    pub(crate) fn bounded(self, max_width_px: f32, halign: HAlign, fit: LineFit) -> Self {
        Self {
            text: self.text,
            key: self.key.bounded(max_width_px, halign, fit),
        }
    }

    pub(crate) fn unbounded_version(self) -> Self {
        Self {
            text: self.text,
            key: self.key.unbounded_version(),
        }
    }
}

impl Default for TextShaper {
    fn default() -> Self {
        Self::new()
    }
}

impl TextShaper {
    /// Cosmic-backed shaper with the bundled fonts loaded. The shaper's
    /// shaped-buffer cache is shared across all clones of this handle.
    pub fn new() -> Self {
        Self {
            inner: Rc::new(RefCell::new(ShaperInner::new(Some(
                CosmicMeasure::with_bundled_fonts(),
            )))),
        }
    }

    /// Shape `request` once and lease its measurement + geometry
    /// queries. The probe holds the shaper's exclusive borrow until
    /// dropped, so its buffer-backed queries stay coherent with the
    /// measurement.
    pub(crate) fn layout<'t>(&self, request: TextShapeRequest<'t>) -> TextLayoutProbe<'_, 't> {
        let mut inner = self.inner.borrow_mut();
        let size = if request.text.is_empty() {
            Size::ZERO
        } else {
            inner.dispatch(request).size
        };
        TextLayoutProbe::new(size, request, inner)
    }

    /// Shape a run at its natural width. `TextSystem` calls this on a
    /// reuse-slot miss; the shaper's own content cache may still hit, so
    /// this is "no reuse slot", not "reshape".
    pub(crate) fn shape_root(&self, request: TextShapeRequest<'_>) -> TextRoot {
        debug_assert!(
            request.key.max_width_px().is_none(),
            "a root shape must be unbounded",
        );
        self.inner.borrow_mut().dispatch(request)
    }

    /// Shape a run against a committed width. Only its extent survives —
    /// a bounded shape has no wrapping floor of its own and its line count
    /// describes the resolve, not the run.
    pub(crate) fn shape_bounded(&self, request: TextShapeRequest<'_>) -> Size {
        debug_assert!(
            request.key.max_width_px().is_some(),
            "a bounded shape needs a committed width",
        );
        self.inner.borrow_mut().dispatch(request).size
    }

    /// Whether this shaper produces shaped buffers the renderer can replay.
    /// False only under the `internals`-gated mono metric, whose runs carry
    /// [`TextShapeKey::INVALID`] so the encoder drops them.
    pub(crate) fn shapes_buffers(&self) -> bool {
        self.inner.borrow().cosmic.is_some()
    }

    /// Bounds the reconstructible cosmic buffer LRU. Called by
    /// `TextSystem::end_frame`; no-op on the mono fallback.
    fn end_frame(&self) {
        self.inner.borrow_mut().end_frame();
    }

    /// Exclusive render-side lease for one batch's encoded-cache misses:
    /// glyph extraction (restoring evicted buffers on the way) and
    /// rasterization, all in aperture-native terms — see
    /// [`crate::text::render`].
    pub(crate) fn render_session(&self) -> TextRenderSession<'_> {
        TextRenderSession::new(RefMut::map(self.inner.borrow_mut(), |inner| {
            inner
                .cosmic
                .as_mut()
                .expect("text render sessions require a cosmic text shaper")
        }))
    }
}

impl ShaperInner {
    /// Bound the ordinary content cache. Layout and reuse entries may retain
    /// evicted keys because the encoder reconstructs every emitted run.
    fn end_frame(&mut self) {
        if let Some(cosmic) = self.cosmic.as_mut() {
            cosmic.end_frame_evict(BUFFER_BUDGET);
        }
    }

    /// Bypass-cache dispatch. Test builds tally it into `measure_calls` —
    /// cosmic may still hit its shaped-buffer cache, so the counter tracks
    /// dispatches, not reshapes.
    fn dispatch(&mut self, request: TextShapeRequest<'_>) -> TextRoot {
        #[cfg(any(test, feature = "internals"))]
        {
            self.measure_calls += 1;
        }
        match self.cosmic.as_mut() {
            Some(cosmic) => cosmic.shape(request),
            #[cfg(any(test, feature = "internals"))]
            None => mono::internals::measure(request),
            // The mono metric is gated out of production, and so is the
            // only constructor that could put us here.
            #[cfg(not(any(test, feature = "internals")))]
            None => unreachable!("the mono fallback needs a test or internals build"),
        }
    }
}

/// A run's *unbounded* shape — the root every wrap policy reasons from.
///
/// Carries the two facts only an unbounded shape can supply, which is why
/// they live here and not on the width-resolved result: a bounded shape
/// cannot report a wrapping floor it never scanned for, and its line count
/// answers a different question. Nothing here identifies a shaped buffer;
/// the buffer key is derived by [`TextSystem`](crate::text::system::TextSystem)
/// from the request that produced it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct TextRoot {
    size: Size,
    /// Width of the widest unbreakable run (typically the longest word).
    /// The wrapping path uses this as the floor when a parent commits a
    /// narrower width: text overflows rather than breaking inside a word.
    intrinsic_min: f32,
    /// `true` when the shaped result is one visual line. Gates
    /// `TextSystem::measure`'s fitting-truncate skip: a single-line run
    /// whose natural width fits the committed width needs no Clip/Ellipsis
    /// resolve — the unbounded root stands in.
    single_line: bool,
}

impl TextRoot {
    /// Successful empty-text shape.
    const ZERO: Self = Self {
        size: Size::ZERO,
        intrinsic_min: 0.0,
        single_line: true,
    };
}

#[cfg(any(test, feature = "internals"))]
pub(crate) mod internals {
    #![allow(dead_code)]
    use crate::text::key::TextShapeKey;
    use crate::text::probe::{CursorPos, TextLayoutProbe};
    use crate::text::wrap::LineFit;
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
        pub(crate) fn unbounded_request<'a>(self, text: &'a str) -> TextShapeRequest<'a> {
            TextShapeRequest::unbounded(
                text,
                self.font_size_px,
                self.line_height_px,
                self.family,
                self.weight,
            )
        }

        pub(crate) fn request<'a>(self, text: &'a str, fit: LineFit) -> TextShapeRequest<'a> {
            let request = self.unbounded_request(text);
            match self.max_width_px {
                Some(width) => request.bounded(width, self.halign, fit),
                None => request,
            }
        }
    }

    /// Shaping result as the in-tree tests read it: a production
    /// [`TextRoot`] plus the shaped-buffer key its request minted.
    /// Production derives that key in
    /// [`TextSystem`](crate::text::system::TextSystem) rather than carrying
    /// it on the measurement, but tests assert on buffer identity, so the
    /// helpers hand both back together.
    #[derive(Clone, Copy, Debug)]
    pub(crate) struct TestMeasure {
        pub(crate) size: Size,
        pub(crate) key: TextShapeKey,
        pub(crate) intrinsic_min: f32,
        pub(crate) single_line: bool,
    }

    impl TestMeasure {
        fn new(root: TextRoot, key: TextShapeKey) -> Self {
            Self {
                size: root.size,
                key,
                intrinsic_min: root.intrinsic_min,
                single_line: root.single_line,
            }
        }
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
            TestMeasure::new(self.shape(request), key)
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
    }

    impl TextShaper {
        pub(crate) fn measure(&self, text: &str, shape: TestShape) -> TestMeasure {
            let shapes_buffers = self.shapes_buffers();
            self.probe_layout(text, shape, |probe| {
                let key = if shapes_buffers && !probe.request.text.is_empty() {
                    probe.request.key
                } else {
                    TextShapeKey::INVALID
                };
                // The probe keeps only the extent; the wrap floor and line
                // count are the root's, so re-derive them for the tests
                // that assert on them.
                TestMeasure {
                    size: probe.size,
                    key,
                    intrinsic_min: 0.0,
                    single_line: true,
                }
            })
        }

        pub(crate) fn probe_layout<R>(
            &self,
            text: &str,
            shape: TestShape,
            body: impl FnOnce(TextLayoutProbe<'_, '_>) -> R,
        ) -> R {
            body(self.layout(shape.request(text, LineFit::Wrap)))
        }

        pub(crate) fn cursor_xy(
            &self,
            text: &str,
            byte_offset: usize,
            shape: TestShape,
        ) -> CursorPos {
            self.probe_layout(text, shape, |probe| probe.cursor_xy(byte_offset))
        }

        pub(crate) fn byte_at_xy(&self, text: &str, x: f32, y: f32, shape: TestShape) -> usize {
            self.probe_layout(text, shape, |probe| probe.byte_at_xy(x, y))
        }

        /// Deterministic mono-fallback shaper for tests and headless
        /// tools — no font system, every glyph `font_size_px * 0.5` wide.
        pub fn test_mono() -> Self {
            Self {
                inner: Rc::new(RefCell::new(ShaperInner::new(None))),
            }
        }

        /// Whether both handles front the same shaped-buffer cache — the
        /// `HostShared` contract that a window's recorder and the backend
        /// never shape into separate caches.
        pub(crate) fn shares_cache_with(&self, other: &Self) -> bool {
            Rc::ptr_eq(&self.inner, &other.inner)
        }

        /// Hold the shaper's exclusive borrow for the caller's scope, so
        /// the backend can prove an encoded-cache hit never reaches for
        /// the shaper: anything that did would panic on the live borrow.
        pub(crate) fn hold_borrow(&self) -> ShaperLease<'_> {
            ShaperLease {
                _inner: self.inner.borrow_mut(),
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

    /// Live exclusive borrow minted by [`TextShaper::hold_borrow`].
    #[derive(Debug)]
    pub(crate) struct ShaperLease<'a> {
        _inner: RefMut<'a, ShaperInner>,
    }
}

#[cfg(test)]
mod tests;
