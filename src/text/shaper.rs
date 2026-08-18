//! The app-global shaping coordinator every window measures through.

use crate::text::cosmic::CosmicMeasure;
use crate::text::glyphs::TextGlyphs;
use crate::text::key::TextShapeKey;
use crate::text::probe::TextProbe;
use crate::text::request::TextShapeRequest;
use crate::text::root::TextRoot;
use crate::text::wrap::WrapFloor;
use std::cell::{RefCell, RefMut};
use std::rc::Rc;

/// Shared, cloneable text shaper. Holds the measurer every window shapes
/// through plus a test/internals-only `measure_calls` counter for
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

/// Which metric a shaper measures with.
///
/// A named pair rather than `Option<CosmicMeasure>`. `None` had to be
/// documented as "the mono fallback" everywhere it was matched, and a
/// shipping build had to carry an `unreachable!` arm for a state its only
/// constructor cannot reach. The mono variant is gated out of production,
/// so a shipping build compiles a one-variant enum — no discriminant, no
/// second arm, and "there is always a measurer" holds by construction
/// rather than by an invariant a comment has to assert.
///
/// `large_enum_variant` asks about the idle variant's footprint, which is
/// the wrong question here: one of these exists per shaper, behind the
/// `Rc<RefCell<…>>`, never in a collection. `allow` and not `expect`,
/// because production gates `Mono` away and leaves the lint nothing to
/// fire on.
#[allow(clippy::large_enum_variant)]
#[derive(Debug)]
enum Metric {
    Cosmic(CosmicMeasure),
    /// The deterministic placeholder — see [`crate::text::mono`]. Built
    /// only by [`TextShaper::test_mono`], which is gated the same way.
    #[cfg(any(test, feature = "internals"))]
    Mono,
}

/// Shared mutable state behind the `Rc<RefCell<...>>` in [`TextShaper`].
/// Both [`crate::Ui`] (layout-time measurement) and
/// [`crate::renderer::backend::WgpuBackend`]
/// (shaping during render) borrow this; backend only touches the measurer
/// via [`TextShaper::glyphs`].
#[derive(Debug)]
pub(super) struct ShaperInner {
    metric: Metric,
    /// **The** frame clock every text cache ages against — the shaped
    /// buffers here, and the encoded-run cache plus glyph atlas in
    /// `renderer::backend::text`, which mirror it through
    /// [`TextShaper::frame`] rather than counting for themselves.
    ///
    /// One counter because the two sides tick on different events and
    /// no pair of separately-incremented counters can be kept in step:
    /// this one advances on the record path
    /// (`TextSystem::end_full_record`, plus the bare clock tick a
    /// `PaintOnly` frame owes through `end_paint_only`), while the
    /// backend's caches are swept on the submit path, which also runs
    /// for `PaintOnly` frames and can run more than once per frame
    /// (offscreen targets). Equal retention *constants* over unequal
    /// clocks bought nothing; see
    /// [`RENDERED_RUN_KEEP_FRAMES`](crate::text::RENDERED_RUN_KEEP_FRAMES).
    ///
    /// Readers must therefore tolerate a clock that jumps (two windows
    /// record before one submit) and one that stalls (two submits
    /// inside one recorded frame). Both are fine for an age comparison;
    /// neither is fine for a cadence gate written as
    /// `frame % INTERVAL == 0`.
    frame: u64,
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
    fn new(metric: Metric) -> Self {
        Self {
            metric,
            frame: 0,
            #[cfg(any(test, feature = "internals"))]
            measure_calls: 0,
        }
    }

    /// The real measurer, or `None` under the gated mono metric — the one
    /// place that question is asked, so no caller matches the field for
    /// itself.
    ///
    /// Reached from [`TextProbe`] too,
    /// which holds this borrow while it answers geometry queries.
    pub(super) fn cosmic(&self) -> Option<&CosmicMeasure> {
        match &self.metric {
            Metric::Cosmic(cosmic) => Some(cosmic),
            #[cfg(any(test, feature = "internals"))]
            Metric::Mono => None,
        }
    }

    /// [`Self::cosmic`], mutably.
    fn cosmic_mut(&mut self) -> Option<&mut CosmicMeasure> {
        match &mut self.metric {
            Metric::Cosmic(cosmic) => Some(cosmic),
            #[cfg(any(test, feature = "internals"))]
            Metric::Mono => None,
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
        match &mut self.metric {
            Metric::Cosmic(cosmic) => cosmic.shape(request, floor),
            #[cfg(any(test, feature = "internals"))]
            Metric::Mono => crate::text::mono::measure(request, floor),
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
            inner: Rc::new(RefCell::new(ShaperInner::new(Metric::Cosmic(
                CosmicMeasure::with_bundled_fonts(),
            )))),
        }
    }

    /// Shape `request` once and lease its measurement + geometry
    /// queries. The probe holds the shaper's exclusive borrow until
    /// dropped, so its buffer-backed queries stay coherent with the
    /// measurement.
    pub(crate) fn layout<'a>(&'a self, request: TextShapeRequest<'a>) -> TextProbe<'a> {
        let mut inner = self.inner.borrow_mut();
        let size = inner.dispatch(request, WrapFloor::Skip).size;
        TextProbe::new(size, request, inner)
    }

    /// Shape `request` and hand back what it measured to. `TextSystem`
    /// calls this on a reuse-slot miss; the shaper's own content cache may
    /// still hit, so this is "no reuse slot", not "reshape".
    ///
    /// Bounded and unbounded alike — which one this is, the request's own
    /// key already says, and both answer a [`TextRoot`]. A bounded resolve
    /// reads only `size` off it: it has no wrapping floor of its own and
    /// its line count describes the resolve rather than the run.
    ///
    /// `floor` opts into the segment scan behind
    /// [`TextRoot::intrinsic_min`]; only `WrapWithOverflow` reads it, and
    /// it dominates the cost of a shape, so everyone else leaves it off.
    /// Passing [`WrapFloor::Scan`] for a root already shaped without one
    /// backfills it from the resident buffer rather than reshaping. The
    /// floor belongs to the unbounded root, so asking for it against a
    /// committed width is a wiring bug — the same contract
    /// `CosmicMeasure::measure_wrapped` asserts on the other side.
    pub(super) fn shape(&self, request: TextShapeRequest<'_>, floor: WrapFloor) -> TextRoot {
        debug_assert!(
            floor == WrapFloor::Skip || request.key.max_width_px().is_none(),
            "the wrap floor is a property of the unbounded root",
        );
        self.inner.borrow_mut().dispatch(request, floor)
    }

    /// Report that `key` is no longer reachable through the reuse slot
    /// that owned it, so its buffer ages on the short window instead of
    /// the long one. `TextSystem` is the only caller — it holds the
    /// slot table that makes the distinction — and the mono fallback
    /// shapes no buffers, so this is a no-op there.
    pub(crate) fn supersede(&self, key: TextShapeKey) {
        if let Some(cosmic) = self.inner.borrow_mut().cosmic_mut() {
            cosmic.supersede(key);
        }
    }

    /// Whether this shaper produces shaped buffers the renderer can replay.
    /// False only under the `internals`-gated mono metric, whose runs carry
    /// [`TextShapeKey::INVALID`] so the encoder drops them.
    pub(crate) fn shapes_buffers(&self) -> bool {
        self.inner.borrow().cosmic().is_some()
    }

    /// Advance the shared frame clock and age out the ordinary content
    /// cache. Called by both of `TextSystem`'s frame teardowns, so it runs
    /// once per window per recorded frame; the cache sweep is a no-op on
    /// the mono fallback but the clock ticks either way, since the
    /// backend's caches read it through [`Self::frame`].
    ///
    /// Layout and reuse entries may retain dropped keys because the
    /// encoder reconstructs every emitted run. The tick lives here rather
    /// than beside the backend's sweep so a headless `Ui` — and every
    /// `TextSystem` test — ages text the same way a presenting window
    /// does.
    ///
    /// `tick_frame`, not `end_frame`, because this is the one production
    /// method that *advances* the clock. Everything downstream — the
    /// shaped-buffer cache, the glyph atlas, the encoded-run cache —
    /// receives it as an `end_frame(frame)` argument instead, and the
    /// two names are what say which side of that line a method is on.
    pub(crate) fn tick_frame(&self) {
        let mut inner = self.inner.borrow_mut();
        inner.frame += 1;
        let frame = inner.frame;
        if let Some(cosmic) = inner.cosmic_mut() {
            cosmic.end_frame(frame);
        }
    }

    /// The current value of the shared frame clock — see
    /// [`ShaperInner::frame`]. The renderer's glyph atlas and
    /// encoded-run cache stamp and expire against this rather than
    /// counting frames of their own, which is what keeps their retention
    /// window and the shaped-buffer cache's measured in the same unit.
    pub(crate) fn frame(&self) -> u64 {
        self.inner.borrow().frame
    }

    /// Lay glyphs out and rasterize them directly: the exclusive render-side
    /// lease, in palantir-native terms — [`PlacedGlyph`](crate::PlacedGlyph)
    /// placements and [`GlyphImage`](crate::GlyphImage) bitmaps, with no
    /// cosmic type in sight.
    ///
    /// **The one lease, taken by both sides.** Palantir's own text backend
    /// holds it for a batch of encoded-cache misses; a caller drawing its own
    /// text — a [`GpuView`](crate::GpuView) labelling a 3D scene, say — holds
    /// it for as long as it is laying out. Holding one across a call that
    /// measures text — anything on [`Ui`](crate::Ui) that lays out a widget —
    /// would ask this `RefCell` for a second borrow and panic, so a view takes
    /// one inside its own paint and drops it there.
    ///
    /// Reached through [`GpuInitCtx`](crate::GpuInitCtx), which hands a view the
    /// shaper the rest of the window is already drawing with — so a label in a
    /// scene is in the same faces as the UI around it without anyone arranging
    /// for that.
    pub fn glyphs(&self) -> TextGlyphs<'_> {
        TextGlyphs::new(RefMut::map(self.inner.borrow_mut(), |inner| {
            inner
                .cosmic_mut()
                .expect("laying glyphs out requires a cosmic text shaper")
        }))
    }
}

#[cfg(any(test, feature = "internals"))]
pub(crate) mod internals {
    #![allow(dead_code)]
    use super::*;
    use crate::layout::ShapedText;
    #[cfg(test)]
    use crate::text::cosmic::counters::CacheCounts;
    use crate::text::probe::Caret;
    use crate::text::request::internals::TestShape;
    use crate::text::wrap::LineFit;

    impl TextShaper {
        /// Everything a layout probe can answer: the extent, and the key
        /// of the buffer it shaped under — which is a [`ShapedText`], the
        /// same pair layout carries out of `TextSystem::measure`, so it
        /// is that rather than a second spelling of it.
        ///
        /// Deliberately not a [`TestMeasure`](crate::text::root::internals::TestMeasure):
        /// the probe keeps only the
        /// extent — the wrap floor and the line count are the root's — so
        /// handing one back meant inventing two of its four fields, and a
        /// test that read one was pinning the invention rather than the
        /// shaper.
        pub(crate) fn measure(&self, text: &str, shape: TestShape) -> ShapedText {
            let shapes_buffers = self.shapes_buffers();
            self.probe_layout(text, shape, |probe| ShapedText {
                measured: probe.size(),
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
            body: impl FnOnce(TextProbe<'_>) -> R,
        ) -> R {
            body(self.layout(shape.request(text, LineFit::Wrap)))
        }

        pub(crate) fn cursor_xy(&self, text: &str, byte_offset: usize, shape: TestShape) -> Caret {
            self.probe_layout(text, shape, |probe| probe.caret_at(byte_offset))
        }

        pub(crate) fn byte_at_xy(&self, text: &str, x: f32, y: f32, shape: TestShape) -> usize {
            self.probe_layout(text, shape, |probe| probe.byte_at(x, y))
        }

        /// Deterministic mono-fallback shaper for tests and headless
        /// tools — no font system, every glyph `font_size_px * 0.5` wide.
        pub fn test_mono() -> Self {
            Self {
                inner: Rc::new(RefCell::new(ShaperInner::new(Metric::Mono))),
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
                .cosmic()
                .map_or(0, CosmicMeasure::cache_len)
        }

        /// Snapshot of the shaped-buffer cache's tallies.
        #[cfg(test)]
        pub(crate) fn cache_counts(&self) -> CacheCounts {
            self.inner
                .borrow()
                .cosmic()
                .expect("cache counts require a cosmic text shaper")
                .counters
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
            if let Some(cosmic) = self.inner.borrow_mut().cosmic_mut() {
                cosmic.ensure_buffer(request);
            }
        }

        pub(crate) fn has_cosmic_buffer(&self, key: TextShapeKey) -> bool {
            self.inner
                .borrow()
                .cosmic()
                .is_some_and(|cosmic| cosmic.shaped_run(key).is_some())
        }

        /// Drop every shaped buffer now — see
        /// [`CosmicMeasure::drop_all_buffers`].
        pub(crate) fn drop_cosmic_buffers(&self) {
            self.inner
                .borrow_mut()
                .cosmic_mut()
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
