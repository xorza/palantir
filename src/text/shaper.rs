//! The app-global shaping coordinator every window measures through.

use crate::primitives::size::Size;
use crate::text::cosmic::CosmicMeasure;
use crate::text::key::TextShapeKey;
use crate::text::layout_probe::TextLayoutProbe;
use crate::text::render::TextRenderSession;
use crate::text::request::TextShapeRequest;
use crate::text::root::TextRoot;
use crate::text::wrap::WrapFloor;
use std::cell::{RefCell, RefMut};
use std::rc::Rc;

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

/// Shared mutable state behind the `Rc<RefCell<...>>` in [`TextShaper`].
/// Both [`crate::Ui`] (layout-time measurement) and
/// [`crate::renderer::backend::WgpuBackend`]
/// (shaping during render) borrow this; backend only touches `cosmic` via
/// [`TextShaper::render_session`].
#[derive(Debug)]
pub(crate) struct ShaperInner {
    /// `None` ⇒ the test/internals-only mono fallback. `TextShaper::new`
    /// always installs `Some`, and `TextShaper::test_mono` is the only
    /// `None` construction, so production never observes it.
    ///
    /// Visible to the module tree because
    /// [`TextLayoutProbe`](crate::text::layout_probe::TextLayoutProbe)
    /// reads it through the borrow it holds.
    pub(super) cosmic: Option<CosmicMeasure>,
    /// Total [`ShaperInner::dispatch`] calls: `TextSystem` reuse misses
    /// plus every bypass [`TextShaper::layout`] call —
    /// which may still hit the cosmic buffer cache, so this counts
    /// dispatches, not reshapes. Reuse-slot hits don't increment.
    /// Read by tests pinning reshape-skip behaviour via
    /// [`TextShaper::measure_calls`]; production builds carry neither
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

    /// Age out the ordinary content cache. Layout and reuse entries may
    /// retain dropped keys because the encoder reconstructs every emitted
    /// run.
    fn end_frame(&mut self) {
        if let Some(cosmic) = self.cosmic.as_mut() {
            cosmic.end_frame();
        }
    }

    /// Bypass-cache dispatch. Test builds tally it into `measure_calls` —
    /// cosmic may still hit its shaped-buffer cache, so the counter tracks
    /// dispatches, not reshapes.
    ///
    /// Empty text answers here, ahead of the tally, so no shaper entry
    /// point needs a guard of its own and a run with nothing to shape
    /// never reads as a dispatch. Both backends keep their own guard for
    /// the callers that reach them directly.
    fn dispatch(&mut self, request: TextShapeRequest<'_>, floor: WrapFloor) -> TextRoot {
        if request.text.is_empty() {
            return TextRoot::ZERO;
        }
        #[cfg(any(test, feature = "internals"))]
        {
            self.measure_calls += 1;
        }
        match self.cosmic.as_mut() {
            Some(cosmic) => cosmic.shape(request, floor),
            #[cfg(any(test, feature = "internals"))]
            None => crate::text::mono::internals::measure(request, floor),
            // The mono metric is gated out of production, and so is the
            // only constructor that could put us here.
            #[cfg(not(any(test, feature = "internals")))]
            None => unreachable!("the mono fallback needs a test or internals build"),
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
        let size = inner.dispatch(request, WrapFloor::Skip).size;
        TextLayoutProbe::new(size, request, inner)
    }

    /// Shape a run at its natural width. `TextSystem` calls this on a
    /// reuse-slot miss; the shaper's own content cache may still hit, so
    /// this is "no reuse slot", not "reshape".
    ///
    /// `floor` opts into the segment scan behind
    /// [`TextRoot::intrinsic_min`]; only `WrapWithOverflow` reads it, and
    /// it dominates the cost of a shape, so everyone else leaves it off.
    /// Passing [`WrapFloor::Scan`] for a root already shaped without one
    /// backfills it from the resident buffer rather than reshaping.
    pub(crate) fn shape_root(&self, request: TextShapeRequest<'_>, floor: WrapFloor) -> TextRoot {
        debug_assert!(
            request.key.max_width_px().is_none(),
            "a root shape must be unbounded",
        );
        self.inner.borrow_mut().dispatch(request, floor)
    }

    /// Shape a run against a committed width. Only its extent survives —
    /// a bounded shape has no wrapping floor of its own and its line count
    /// describes the resolve, not the run.
    pub(crate) fn shape_bounded(&self, request: TextShapeRequest<'_>) -> Size {
        debug_assert!(
            request.key.max_width_px().is_some(),
            "a bounded shape needs a committed width",
        );
        self.inner
            .borrow_mut()
            .dispatch(request, WrapFloor::Skip)
            .size
    }

    /// Report that `key` is no longer reachable through the reuse slot
    /// that owned it, so its buffer ages on the short window instead of
    /// the long one. `TextSystem` is the only caller — it holds the
    /// slot table that makes the distinction — and the mono fallback
    /// shapes no buffers, so this is a no-op there.
    pub(crate) fn supersede(&self, key: TextShapeKey) {
        if let Some(cosmic) = self.inner.borrow_mut().cosmic.as_mut() {
            cosmic.supersede(key);
        }
    }

    /// Whether this shaper produces shaped buffers the renderer can replay.
    /// False only under the `internals`-gated mono metric, whose runs carry
    /// [`TextShapeKey::INVALID`] so the encoder drops them.
    pub(crate) fn shapes_buffers(&self) -> bool {
        self.inner.borrow().cosmic.is_some()
    }

    /// Bounds the reconstructible cosmic buffer LRU. Called by
    /// `TextSystem::end_frame`; no-op on the mono fallback.
    pub(super) fn end_frame(&self) {
        self.inner.borrow_mut().end_frame();
    }

    /// Exclusive render-side lease for one batch's encoded-cache misses:
    /// glyph extraction (restoring evicted buffers on the way) and
    /// rasterization, all in palantir-native terms — see
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

#[cfg(any(test, feature = "internals"))]
pub(crate) mod internals {
    #![allow(dead_code)]
    use super::*;
    #[cfg(test)]
    use crate::text::cache_probe::CacheCounts;
    use crate::text::layout_probe::Caret;
    use crate::text::request::internals::TestShape;
    use crate::text::wrap::LineFit;

    /// Everything a layout probe can answer: the extent, and the key of
    /// the buffer it shaped under.
    ///
    /// Deliberately *not* a [`TestMeasure`]. The probe keeps only the
    /// extent — the wrap floor and the line count are the root's — so
    /// handing back a `TestMeasure` meant inventing two of its four
    /// fields, and a test that read one was pinning the invention rather
    /// than the shaper. Two fields, both real.
    #[derive(Clone, Copy, Debug)]
    pub(crate) struct ProbeMeasure {
        pub(crate) size: Size,
        pub(crate) key: TextShapeKey,
    }

    impl TextShaper {
        pub(crate) fn measure(&self, text: &str, shape: TestShape) -> ProbeMeasure {
            let shapes_buffers = self.shapes_buffers();
            self.probe_layout(text, shape, |probe| ProbeMeasure {
                size: probe.size,
                key: if shapes_buffers && !probe.request.text.is_empty() {
                    probe.request.key
                } else {
                    TextShapeKey::INVALID
                },
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

        pub(crate) fn cursor_xy(&self, text: &str, byte_offset: usize, shape: TestShape) -> Caret {
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

        /// Shaped buffers currently resident.
        pub(crate) fn cosmic_cache_len(&self) -> usize {
            self.inner
                .borrow()
                .cosmic
                .as_ref()
                .map_or(0, CosmicMeasure::cache_len)
        }

        /// Snapshot of the shaped-buffer cache's tallies.
        #[cfg(test)]
        pub(crate) fn cache_counts(&self) -> CacheCounts {
            self.inner
                .borrow()
                .cosmic
                .as_ref()
                .expect("cache counts require a cosmic text shaper")
                .probe
                .counts()
        }

        /// The lookup `TextEncoder::encode_run` performs on an
        /// encoded-cache miss: restore the shaped buffer if it aged out,
        /// and promote it onto the protected window if it is resident.
        ///
        /// Tests that model a *rendered* frame need this. Layout only
        /// ever inserts, so without the render half a buffer is never
        /// looked up and the protected window is unreachable — which is
        /// exactly the asymmetry `PROBATION_KEEP_FRAMES` documents.
        pub(crate) fn render_ensure(&self, request: TextShapeRequest<'_>) {
            if let Some(cosmic) = self.inner.borrow_mut().cosmic.as_mut() {
                cosmic.ensure_buffer(request);
            }
        }

        pub(crate) fn has_cosmic_buffer(&self, key: TextShapeKey) -> bool {
            self.inner
                .borrow()
                .cosmic
                .as_ref()
                .is_some_and(|cosmic| cosmic.shaped_run(key).is_some())
        }

        /// Drop every shaped buffer now — see
        /// [`CosmicMeasure::drop_all_buffers`].
        pub(crate) fn drop_cosmic_buffers(&self) {
            self.inner
                .borrow_mut()
                .cosmic
                .as_mut()
                .expect("cosmic buffer eviction requires a cosmic text shaper")
                .drop_all_buffers();
        }
    }

    /// Live exclusive borrow minted by [`TextShaper::hold_borrow`].
    #[derive(Debug)]
    pub(crate) struct ShaperLease<'a> {
        _inner: RefMut<'a, ShaperInner>,
    }
}
