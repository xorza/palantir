//! The app-global shaping coordinator every window measures through.

use crate::primitives::geometry::size::Size;
use crate::text::cosmic::CosmicMeasure;
use crate::text::error::FontLoadError;
use crate::text::extent::TextExtent;
use crate::text::font_family::FontFamily;
use crate::text::font_scope::FontScope;
use crate::text::font_source::FontSource;
use crate::text::glyphs::TextGlyphs;
use crate::text::key::TextShapeKey;
use crate::text::probe::TextProbe;
use crate::text::request::TextShapeRequest;
use crate::text::root::TextRoot;
use crate::text::run::TextRun;
use crate::text::wrap::{WrapCommit, WrapFloor};
use std::cell::{Cell, RefCell, RefMut};
use std::rc::Rc;

/// Shared, cloneable text shaper: the measurer every window shapes through.
/// Single-threaded (`Rc` inside); the `RefCell` only guards against re-entry.
/// Cloning bumps a refcount.
///
/// Construct with [`Self::new`] / `Default` (bundled fonts only) or
/// [`Self::with_fonts`]. Test and internals builds add `Self::test_mono`.
#[derive(Clone, Debug)]
pub struct TextShaper {
    shared: Rc<Shared>,
}

/// What every clone fronts. The font epoch sits beside the `RefCell` because the
/// encoded-run cache checks it before every batch and an all-hit frame must never
/// crack the borrow.
#[derive(Debug)]
struct Shared {
    inner: RefCell<ShaperInner>,
    font_epoch: Cell<u32>,
}

/// Shared state behind the `Rc<RefCell<...>>` in [`TextShaper`], borrowed by
/// [`crate::Ui`] (layout) and the wgpu backend (render).
#[derive(Debug)]
pub(super) struct ShaperInner {
    /// The measurer and owner of the shared frame clock ([`CosmicMeasure::frame`]).
    /// Held outright: a shaper always has a font system, mono included.
    cosmic: CosmicMeasure,
    /// Measure through the deterministic mono metric ([`crate::text::mono`]); only
    /// [`TextShaper::test_mono`] sets it. [`Self::root`] and [`Self::resolve`] mint no
    /// buffer, so the renderer drops mono runs.
    #[cfg(any(test, feature = "internals"))]
    mono: bool,
    /// Total dispatches: `TextSystem` reuse misses plus every bypass
    /// [`TextShaper::layout`] call (the cosmic cache may still hit). Test builds only.
    #[cfg(any(test, feature = "internals"))]
    measure_calls: u64,
}

impl ShaperInner {
    const fn new(cosmic: CosmicMeasure) -> Self {
        Self {
            cosmic,
            #[cfg(any(test, feature = "internals"))]
            mono: false,
            #[cfg(any(test, feature = "internals"))]
            measure_calls: 0,
        }
    }

    pub(super) const fn cosmic(&self) -> &CosmicMeasure {
        &self.cosmic
    }

    /// Whether measurement takes the mono metric; a literal `false` in production.
    pub(super) const fn is_mono(&self) -> bool {
        #[cfg(any(test, feature = "internals"))]
        {
            self.mono
        }
        #[cfg(not(any(test, feature = "internals")))]
        {
            false
        }
    }

    /// The run's unbounded shape. `floor` opts into the segment scan behind
    /// [`TextRoot::intrinsic_min`].
    pub(super) fn root(&mut self, request: TextShapeRequest<'_>, floor: WrapFloor) -> TextRoot {
        self.tally_dispatch();
        #[cfg(any(test, feature = "internals"))]
        #[expect(
            clippy::absolute_paths,
            reason = "a gated statement names the path inline instead of a cfg'd import"
        )]
        if self.mono {
            return crate::text::mono::root(request, floor);
        }
        self.cosmic.root(request, floor)
    }

    /// The extent at the width its key commits. The bounded half takes no `floor`.
    pub(super) fn resolve(&mut self, request: TextShapeRequest<'_>) -> TextExtent {
        self.tally_dispatch();
        #[cfg(any(test, feature = "internals"))]
        #[expect(
            clippy::absolute_paths,
            reason = "a gated statement names the path inline instead of a cfg'd import"
        )]
        if self.mono {
            return crate::text::mono::resolve(request);
        }
        self.cosmic.resolve(request)
    }

    #[inline]
    const fn tally_dispatch(&mut self) {
        #[cfg(any(test, feature = "internals"))]
        {
            self.measure_calls += 1;
        }
    }
}

impl Default for TextShaper {
    fn default() -> Self {
        Self::new()
    }
}

impl TextShaper {
    /// Cosmic-backed shaper over the bundled faces alone: deterministic metrics, no
    /// font directory walk. A window chooses otherwise through
    /// [`WinitHostBuilder::fonts`](crate::WinitHostBuilder::fonts).
    pub fn new() -> Self {
        Self::with_fonts(FontScope::Bundled)
    }

    /// Cosmic-backed shaper over the faces `scope` names.
    pub fn with_fonts(scope: FontScope) -> Self {
        Self::over(CosmicMeasure::new(scope))
    }

    /// The shaper around a measurer built elsewhere (`FontScan::join`).
    pub(super) fn over(measure: CosmicMeasure) -> Self {
        Self {
            shared: Rc::new(Shared {
                inner: RefCell::new(ShaperInner::new(measure)),
                font_epoch: Cell::new(0),
            }),
        }
    }

    /// Register every face in `source` and return the family of the first; see
    /// [`Ui::load_font`](crate::Ui::load_font).
    ///
    /// # Errors
    ///
    /// [`FontLoadError::Io`] for an unreadable file, [`FontLoadError::NoFaces`] for
    /// bytes with no face, [`FontLoadError::FamilyTableFull`] for a full family table.
    pub fn load_font(&self, source: impl Into<FontSource>) -> Result<FontFamily, FontLoadError> {
        let loaded = self
            .shared
            .inner
            .borrow_mut()
            .cosmic
            .load_font(source.into())?;
        self.shared.font_epoch.set(self.shared.font_epoch.get() + 1);
        Ok(loaded)
    }

    /// Whether a face answers to `family`.
    pub fn has_font(&self, family: FontFamily) -> bool {
        self.shared.inner.borrow_mut().cosmic.has_font(family)
    }

    /// Every family the database knows, system fonts included.
    pub fn font_families(&self) -> Vec<FontFamily> {
        self.shared.inner.borrow().cosmic.font_families()
    }

    /// How many times [`Self::load_font`] has changed the database. Every cache keyed
    /// on a resolved face owes this a comparison: a load changes which face a family
    /// resolves to, but a [`TextShapeKey`] carries only the family index. Read by the
    /// encoded-run cache and by `TextSystem::sync_fonts`; shaped buffers are dropped by
    /// [`CosmicMeasure::load_font`].
    pub(crate) fn font_epoch(&self) -> u32 {
        self.shared.font_epoch.get()
    }

    /// Shape `run` once and lease its measurement and geometry queries; the probe
    /// holds the exclusive borrow until dropped. The width is resolved here, as
    /// `TextSystem::measure` does, because both steps need the unbounded root; doing
    /// it elsewhere mints a key layout never shaped and the caret answers against a
    /// buffer wrapped at another width.
    pub(crate) fn layout<'a>(&'a self, run: &TextRun<'a>) -> TextProbe<'a> {
        let mut inner = self.shared.inner.borrow_mut();
        let halign = run.align.halign();
        let Some(unbounded) = run.unbounded_request() else {
            // Nothing to shape (no bytes, or a face with no usable size): the block is empty
            // at its own origin.
            return TextProbe::new(Size::ZERO, run.text, run.unbounded_key(), halign, inner);
        };
        let (key, size) = match (run.wrap_width(), run.wrap.line_fit()) {
            (Some(width), Some(fit)) => {
                let floor = run.wrap.floor_scan();
                match run
                    .wrap
                    .commit(width, halign, fit, || inner.root(unbounded, floor))
                {
                    WrapCommit::Unbounded { extent } => (unbounded.key, extent.size),
                    WrapCommit::Bound(bound) => {
                        let bound = unbounded.with_bound(bound);
                        (bound.key, inner.resolve(bound).size)
                    }
                }
            }
            _ => (
                unbounded.key,
                inner.root(unbounded, WrapFloor::Skip).extent.size,
            ),
        };
        TextProbe::new(size, run.text, Some(key), halign, inner)
    }

    /// The run's unbounded shape; `TextSystem` calls this on a reuse-slot miss. `floor`
    /// opts into the segment scan behind [`TextRoot::intrinsic_min`] (only
    /// `WrapWithOverflow` reads it; it dominates shape cost). [`WrapFloor::Scan`] on a
    /// root shaped without one backfills from the resident buffer.
    pub(super) fn root(&self, request: TextShapeRequest<'_>, floor: WrapFloor) -> TextRoot {
        self.shared.inner.borrow_mut().root(request, floor)
    }

    /// The extent at the width its key commits: the bounded half of [`Self::root`].
    pub(super) fn resolve(&self, request: TextShapeRequest<'_>) -> TextExtent {
        self.shared.inner.borrow_mut().resolve(request)
    }

    /// Report that `key` is no longer reachable through the reuse slot that owned it,
    /// so its buffer ages on the short window. Silent for a non-resident key (every
    /// key under mono).
    pub(crate) fn supersede(&self, key: TextShapeKey) {
        self.shared.inner.borrow_mut().cosmic.supersede(key);
    }

    /// Whether this shaper produces buffers the renderer can replay; false under mono.
    pub(crate) fn shapes_buffers(&self) -> bool {
        !self.shared.inner.borrow().is_mono()
    }

    /// Advance the shared frame clock and age out the shaped-buffer cache (see
    /// [`CosmicMeasure::tick_frame`]). Every host frame owes this exactly once, even
    /// one that records nothing (`FrameRuntime::tick_text_clock`): a stalled clock
    /// leaves a full glyph atlas unable to evict, so glyphs go missing.
    pub(crate) fn tick_frame(&self) {
        self.shared.inner.borrow_mut().cosmic.tick_frame();
    }

    /// The shared frame clock ([`CosmicMeasure::frame`]); the glyph atlas and
    /// encoded-run cache expire against it.
    pub(crate) fn frame(&self) -> u64 {
        self.shared.inner.borrow().cosmic.frame()
    }

    /// Lay glyphs out and rasterize them directly: the exclusive render-side lease,
    /// in [`PlacedGlyph`](crate::widget::PlacedGlyph) and
    /// [`RasterImage`](crate::widget::RasterImage) terms. Real glyphs under mono too.
    ///
    /// Palantir's text backend and a caller drawing its own text (a
    /// [`GpuView`](crate::GpuView) labelling a 3D scene) take this one lease. Holding
    /// it across anything on [`Ui`](crate::Ui) that measures text borrows the
    /// `RefCell` twice and panics, so take it inside the view's paint and drop it
    /// there. Reached through [`GpuInitContext`](crate::GpuInitContext).
    pub fn glyphs(&self) -> TextGlyphs<'_> {
        TextGlyphs::new(RefMut::map(self.shared.inner.borrow_mut(), |inner| {
            &mut inner.cosmic
        }))
    }
}

#[cfg(any(test, feature = "internals"))]
pub(crate) mod internals {
    use super::*;
    #[cfg(test)]
    use crate::layout::text::shaped_text::ShapedText;
    #[cfg(test)]
    use crate::primitives::layout::align::Align;
    #[cfg(test)]
    use crate::text::cosmic::counters::CacheCounts;
    #[cfg(test)]
    use crate::text::probe::Caret;
    #[cfg(test)]
    use crate::text::request::internals::TestShape;
    #[cfg(test)]
    use crate::text::wrap::TextWrap;

    impl TextShaper {
        /// Deterministic mono shaper for tests and headless tools: every glyph is
        /// `font_size * 0.5` wide. Only measurement is mono; font loading, family
        /// resolution and [`Self::glyphs`] are real.
        pub(crate) fn test_mono() -> Self {
            let shaper = Self::new();
            shaper.shared.inner.borrow_mut().mono = true;
            shaper
        }
    }

    #[cfg(test)]
    impl TextShaper {
        /// Everything a layout probe can answer: the extent and the key of the buffer it
        /// shaped under ([`ShapedText`]). Not a
        /// [`TestMeasure`](crate::text::root::internals::TestMeasure), which would
        /// invent two of the probe's fields.
        pub(crate) fn measure(&self, text: &str, shape: TestShape) -> ShapedText {
            let mut shaped = self.probe_layout(text, shape, |probe| ShapedText {
                extent: TextExtent::inked_within(probe.size()),
                key: probe.shaped_key(),
            });
            if let Some(key) = shaped.key {
                shaped.extent.ink = self
                    .shared
                    .inner
                    .borrow()
                    .cosmic
                    .cached_extent(key)
                    .expect("a probe's shaped key names a resident buffer")
                    .ink;
            }
            shaped
        }

        /// Describes the fixture as a [`TextRun`] so width binding goes through `layout`.
        pub(crate) fn probe_layout<R>(
            &self,
            text: &str,
            shape: TestShape,
            body: impl FnOnce(TextProbe<'_>) -> R,
        ) -> R {
            body(self.layout(&TextRun {
                text,
                font: shape.font,
                wrap: TextWrap::Wrap,
                align: Align::h(shape.halign),
                max_width: shape.max_width,
            }))
        }

        pub(crate) fn cursor_xy(&self, text: &str, byte_offset: usize, shape: TestShape) -> Caret {
            self.probe_layout(text, shape, |probe| probe.caret_at(byte_offset))
        }

        pub(crate) fn byte_at_xy(&self, text: &str, x: f32, y: f32, shape: TestShape) -> usize {
            self.probe_layout(text, shape, |probe| probe.byte_at(x, y))
        }

        /// Hold the exclusive borrow for the caller's scope, proving an encoded-cache hit
        /// never touches the shaper.
        pub(crate) fn hold_borrow(&self) -> ShaperLease<'_> {
            ShaperLease {
                _inner: self.shared.inner.borrow_mut(),
            }
        }

        pub(crate) fn cache_counts(&self) -> CacheCounts {
            self.shared.inner.borrow().cosmic.cache_counts()
        }

        pub(crate) fn measure_calls(&self) -> u64 {
            self.shared.inner.borrow().measure_calls
        }

        pub(crate) fn has_cosmic_buffer(&self, key: TextShapeKey) -> bool {
            self.shared.inner.borrow().cosmic.shaped_run(key).is_some()
        }

        pub(crate) fn drop_cosmic_buffers(&self) {
            self.shared.inner.borrow_mut().cosmic.drop_all_buffers();
        }
    }

    #[cfg(any(test, feature = "bench"))]
    impl TextShaper {
        pub(crate) fn cosmic_cache_len(&self) -> usize {
            self.shared.inner.borrow().cosmic.cache_len()
        }

        /// The lookup `TextEncoder::encode_run` performs on an encoded-cache miss:
        /// restore an aged-out buffer, promote a resident one. Needed to model a rendered
        /// frame; layout only inserts (`PROBATION_KEEP_FRAMES`).
        pub(crate) fn render_ensure(&self, request: TextShapeRequest<'_>) {
            self.shared.inner.borrow_mut().cosmic.ensure_buffer(request);
        }
    }

    #[cfg(test)]
    #[derive(Debug)]
    pub(crate) struct ShaperLease<'a> {
        _inner: RefMut<'a, ShaperInner>,
    }
}
