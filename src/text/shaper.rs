//! The app-global shaping coordinator every window measures through.

use crate::primitives::size::Size;
use crate::text::cosmic::CosmicMeasure;
use crate::text::error::FontLoadError;
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
/// Construct with [`Self::new`] / `Default` (bundled fonts only), or
/// [`Self::with_fonts`] to say whether the machine's installed fonts
/// come too. Test and internals builds additionally provide
/// `Self::test_mono`, which is one of these over a deterministic
/// measurement metric rather than a shaper of a second kind.
#[derive(Clone, Debug)]
pub struct TextShaper {
    shared: Rc<Shared>,
}

/// What every clone of a handle fronts.
///
/// The font epoch sits *beside* the `RefCell` rather than inside it
/// because the renderer's encoded-run cache checks it before every batch,
/// and an all-hit frame is contracted never to crack that borrow — the
/// text backend's GPU suite holds an exclusive borrow across a prepare to
/// prove exactly that. A `Cell` answers with no borrow at all.
#[derive(Debug)]
struct Shared {
    inner: RefCell<ShaperInner>,
    font_epoch: Cell<u32>,
}

/// Shared mutable state behind the `Rc<RefCell<...>>` in [`TextShaper`].
/// Both [`crate::Ui`] (layout-time measurement) and
/// [`crate::renderer::backend::WgpuBackend`]
/// (shaping during render) borrow this; the backend reaches the measurer
/// through [`TextShaper::glyphs`] and reads its clock through
/// [`TextShaper::frame`].
#[derive(Debug)]
pub(super) struct ShaperInner {
    /// The measurer, and the owner of the shared frame clock — see
    /// [`CosmicMeasure::frame`].
    ///
    /// Held outright, not behind an `Option` or a metric enum. A shaper
    /// always has a font system, [`Self::mono`] included: the mono
    /// metric replaces the *arithmetic* two calls do, and every other
    /// question a shaper answers — which faces exist, what a family
    /// resolves to, what a glyph rasterizes to — is the database's, not
    /// the metric's. Making the measurer optional put that distinction
    /// on eight production call sites as an `expect` or an
    /// `is_some_and`.
    cosmic: CosmicMeasure,
    /// Measure through the deterministic mono metric instead of shaping
    /// — see [`crate::text::mono`], and [`TextShaper::test_mono`], which
    /// is the only thing that sets it.
    ///
    /// A flag rather than a variant, because that is the whole of what
    /// mono is: [`Self::root`] and [`Self::resolve`] answer from
    /// arithmetic and mint no buffer. The database underneath is a real
    /// one, so a mono shaper still loads fonts and still resolves
    /// families; what it does not do is shape, which is why
    /// `TextSystem` leaves its runs with no buffer key and the renderer
    /// drops them.
    #[cfg(any(test, feature = "internals"))]
    mono: bool,
    /// Total [`Self::tally_dispatch`] calls: `TextSystem` reuse misses
    /// plus every bypass [`TextShaper::layout`] call —
    /// which may still hit the cosmic buffer cache, so this counts
    /// dispatches, not reshapes. Reuse-slot hits don't increment.
    /// Read by tests pinning reshape-skip behaviour via
    /// `TextShaper::measure_calls`; production builds carry neither
    /// the field nor the write.
    #[cfg(any(test, feature = "internals"))]
    measure_calls: u64,
}

impl ShaperInner {
    fn new(cosmic: CosmicMeasure) -> Self {
        Self {
            cosmic,
            #[cfg(any(test, feature = "internals"))]
            mono: false,
            #[cfg(any(test, feature = "internals"))]
            measure_calls: 0,
        }
    }

    /// The measurer. Reached from [`TextProbe`] too, which holds this
    /// borrow while it answers geometry queries.
    pub(super) fn cosmic(&self) -> &CosmicMeasure {
        &self.cosmic
    }

    /// Whether measurement takes the mono metric — see [`Self::mono`].
    /// A literal `false` in production, so the two tests that stay
    /// compiled there — [`TextProbe::shaped`] and
    /// [`TextShaper::shapes_buffers`] — fold away rather than reading a
    /// field that could never be set.
    pub(super) fn is_mono(&self) -> bool {
        #[cfg(any(test, feature = "internals"))]
        {
            self.mono
        }
        #[cfg(not(any(test, feature = "internals")))]
        {
            false
        }
    }

    /// The run's **unbounded** shape. `floor` opts into the segment scan
    /// behind [`TextRoot::intrinsic_min`].
    pub(super) fn root(&mut self, request: TextShapeRequest<'_>, floor: WrapFloor) -> TextRoot {
        self.tally_dispatch();
        #[cfg(any(test, feature = "internals"))]
        if self.mono {
            return crate::text::mono::root(request, floor);
        }
        self.cosmic.root(request, floor)
    }

    /// The extent this run resolves to at the width its key commits.
    ///
    /// **The bounded half takes no `floor`**, which is what retires the
    /// contract structural rather than asserted at two layers: a wrapping
    /// floor belongs to the unbounded root, and there is no way to ask a
    /// bounded resolve for one.
    pub(super) fn resolve(&mut self, request: TextShapeRequest<'_>) -> Size {
        self.tally_dispatch();
        #[cfg(any(test, feature = "internals"))]
        if self.mono {
            return crate::text::mono::resolve(request);
        }
        self.cosmic.resolve(request)
    }

    /// Count one bypass-cache dispatch. Test builds only — cosmic may
    /// still hit its shaped-buffer cache, so the counter tracks
    /// dispatches, not reshapes.
    #[inline]
    fn tally_dispatch(&mut self) {
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
    /// Cosmic-backed shaper over the bundled faces alone. The shaper's
    /// shaped-buffer cache is shared across all clones of this handle.
    ///
    /// [`FontScope::Bundled`] rather than `System`, because this is what
    /// a standalone recorder, a golden test and a bench all reach for:
    /// deterministic metrics, and no font directory to walk. A window
    /// says otherwise through
    /// [`WinitHostConfig::fonts`](crate::WinitHostConfig::fonts).
    pub fn new() -> Self {
        Self::with_fonts(FontScope::Bundled)
    }

    /// Cosmic-backed shaper over the faces `scope` names.
    pub fn with_fonts(scope: FontScope) -> Self {
        Self::over(CosmicMeasure::new(scope))
    }

    /// The shaper around a measurer somebody else built — what
    /// `FontScan::join` hands back after the scan it ran on another
    /// thread.
    pub(super) fn over(measure: CosmicMeasure) -> Self {
        Self {
            shared: Rc::new(Shared {
                inner: RefCell::new(ShaperInner::new(measure)),
                font_epoch: Cell::new(0),
            }),
        }
    }

    /// Register every face in `source` and hand back the family of the
    /// first — see [`Ui::load_font`](crate::Ui::load_font), which is how
    /// an app reaches this.
    ///
    /// # Errors
    ///
    /// [`FontLoadError::Io`] for an unreadable file, and
    /// [`FontLoadError::NoFaces`] for bytes that parse to no face.
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
    pub fn font_available(&self, family: FontFamily) -> bool {
        self.shared.inner.borrow_mut().cosmic.font_available(family)
    }

    /// Every family the database knows, system fonts included.
    pub fn font_families(&self) -> Vec<FontFamily> {
        self.shared.inner.borrow().cosmic.font_families()
    }

    /// How many times [`Self::load_font`] has changed the database.
    ///
    /// **Every cache keyed on a resolved face owes this a comparison.** A
    /// load changes which physical face a family resolves to, and a
    /// [`TextShapeKey`] carries the family *index*, not the answer — so
    /// the key of a run that used to fall back to
    /// [`FontFamily::SANS`](crate::FontFamily::SANS) is byte-identical
    /// before and after the face it names arrives. Nothing downstream
    /// can notice on its own.
    ///
    /// Three caches read it, each clearing its own rows: the renderer's
    /// encoded-run cache before it emits a batch
    /// (`TextEncoder::sync_fonts`), and the reuse slots plus the layout
    /// measure cache at the top of a layout run
    /// (`TextSystem::sync_fonts`). The shaped buffers need no such check
    /// — [`CosmicMeasure::load_font`] drops them where it stands, since
    /// it is already holding the borrow.
    pub(crate) fn font_epoch(&self) -> u32 {
        self.shared.font_epoch.get()
    }

    /// Shape `run` once and lease its measurement + geometry queries.
    /// The probe holds the shaper's exclusive borrow until dropped, so
    /// its buffer-backed queries stay coherent with the measurement.
    ///
    /// The width is resolved here rather than by [`TextRun`] itself
    /// because both steps need the run's *unbounded* root, which only a
    /// shaping call produces — the same two steps, in the same order,
    /// that `TextSystem::measure` applies before it binds. Doing them
    /// anywhere else mints a key layout never shaped, and the caret then
    /// answers against a buffer wrapped at a different width than the
    /// one that was drawn.
    pub(crate) fn layout<'a>(&'a self, run: &TextRun<'a>) -> TextProbe<'a> {
        let mut inner = self.shared.inner.borrow_mut();
        // The run's own, not the key's — see `TextProbe::halign`.
        let halign = run.align.halign();
        let Some(unbounded) = run.unbounded_request() else {
            // One of the two crate edges a run with nothing to shape
            // reaches (the other is `TextGlyphs`) — no bytes, or a face
            // with no usable size. Nothing was shaped, so the block is
            // empty and sits at its own origin, which is every answer the
            // probe below can give. The key still carries the metrics it
            // expresses them in, or no key at all where the face named
            // none.
            return TextProbe::new(Size::ZERO, run.text, run.unbounded_key(), halign, inner);
        };
        let (key, size) = match (run.wrap_width(), run.wrap.line_fit()) {
            (Some(width), Some(fit)) => {
                let floor = run.wrap.floor_scan();
                match run
                    .wrap
                    .commit(width, halign, fit, || inner.root(unbounded, floor))
                {
                    WrapCommit::Unbounded { size } => (unbounded.key, size),
                    WrapCommit::Bound(bound) => {
                        let bound = unbounded.with_bound(bound);
                        (bound.key, inner.resolve(bound))
                    }
                }
            }
            _ => (unbounded.key, inner.root(unbounded, WrapFloor::Skip).size),
        };
        TextProbe::new(size, run.text, Some(key), halign, inner)
    }

    /// The run's unbounded shape. `TextSystem` calls this on a reuse-slot
    /// miss; the shaper's own content cache may still hit, so this is "no
    /// reuse slot", not "reshape".
    ///
    /// `floor` opts into the segment scan behind
    /// [`TextRoot::intrinsic_min`]; only `WrapWithOverflow` reads it, and
    /// it dominates the cost of a shape, so everyone else leaves it off.
    /// Passing [`WrapFloor::Scan`] for a root already shaped without one
    /// backfills it from the resident buffer rather than reshaping.
    pub(super) fn root(&self, request: TextShapeRequest<'_>, floor: WrapFloor) -> TextRoot {
        self.shared.inner.borrow_mut().root(request, floor)
    }

    /// The extent this run resolves to at the width its key commits — the
    /// bounded half of [`Self::root`], and the shape a renderer replays.
    pub(super) fn resolve(&self, request: TextShapeRequest<'_>) -> Size {
        self.shared.inner.borrow_mut().resolve(request)
    }

    /// Report that `key` is no longer reachable through the reuse slot
    /// that owned it, so its buffer ages on the short window instead of
    /// the long one. `TextSystem` is the only caller — it holds the
    /// slot table that makes the distinction. Silent on a key no buffer
    /// is resident under, which is every key under the mono metric.
    pub(crate) fn supersede(&self, key: TextShapeKey) {
        self.shared.inner.borrow_mut().cosmic.supersede(key);
    }

    /// Whether this shaper produces shaped buffers the renderer can replay.
    /// False only under the `internals`-gated mono metric, whose runs name
    /// no buffer key so the encoder drops them.
    pub(crate) fn shapes_buffers(&self) -> bool {
        !self.shared.inner.borrow().is_mono()
    }

    /// Advance the shared frame clock and age out the shaped-buffer
    /// cache — see [`CosmicMeasure::tick_frame`], which is where both
    /// happen and which owns the clock.
    ///
    /// Layout and reuse entries may retain dropped keys because the
    /// encoder reconstructs every emitted run. The tick lives here rather
    /// than beside the backend's sweep so a headless `Ui` — and every
    /// `TextSystem` test — ages text the same way a presenting window
    /// does.
    ///
    /// **Every frame owes this exactly once, including one that records
    /// nothing.** `FrameCycle::run` is the one caller, past the arm that
    /// decided what kind of frame this was, so neither plan can skip it
    /// and no plan can pay it twice. Skipping it does more than delay
    /// eviction: the glyph atlas only considers a slot evictable while
    /// `last_use < current_frame`, so a stalled clock leaves a full atlas
    /// unable to reclaim *anything* and every insert starves until a
    /// record frame arrives. That surfaces as glyphs missing from painted
    /// text with no path to recovery.
    pub(crate) fn tick_frame(&self) {
        self.shared.inner.borrow_mut().cosmic.tick_frame();
    }

    /// The current value of the shared frame clock — see
    /// [`CosmicMeasure::frame`]. The renderer's glyph atlas and
    /// encoded-run cache stamp and expire against this rather than
    /// counting frames of their own, which is what keeps their retention
    /// window and the shaped-buffer cache's measured in the same unit.
    pub(crate) fn frame(&self) -> u64 {
        self.shared.inner.borrow().cosmic.frame()
    }

    /// Lay glyphs out and rasterize them directly: the exclusive render-side
    /// lease, in palantir-native terms — [`PlacedGlyph`](crate::PlacedGlyph)
    /// placements and [`RasterImage`](crate::RasterImage) bitmaps, with no
    /// cosmic type in sight.
    ///
    /// Available under the mono metric too, and answers there in real
    /// glyphs: the lease is the *database*'s, and only measurement is
    /// mono. A mono run never reaches it through palantir's own text
    /// backend — it names no buffer key, which the encoder drops — so
    /// the two cannot disagree about one run.
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
        TextGlyphs::new(RefMut::map(self.shared.inner.borrow_mut(), |inner| {
            &mut inner.cosmic
        }))
    }
}

#[cfg(any(test, feature = "internals"))]
pub(crate) mod internals {
    use super::*;
    #[cfg(test)]
    #[cfg(test)]
    use crate::layout::ShapedText;
    #[cfg(test)]
    use crate::layout::types::align::Align;
    #[cfg(test)]
    use crate::text::cosmic::counters::CacheCounts;
    #[cfg(test)]
    use crate::text::probe::Caret;
    #[cfg(test)]
    use crate::text::request::test_support::TestShape;
    #[cfg(test)]
    use crate::text::wrap::TextWrap;

    /// What only an assertion asks: probes that shape a fixture face,
    /// the cache-identity and borrow checks, and the two reach-ins that
    /// read or clear the shaped-buffer cache wholesale. Split from the
    /// block below so the module's gate can stay as wide as the
    /// integration suites need without leaving these dead there.
    #[cfg(test)]
    impl TextShaper {
        /// Everything a layout probe can answer: the extent, and the key
        /// of the buffer it shaped under — which is a [`ShapedText`], the
        /// same pair layout carries out of `TextSystem::measure`, so it
        /// is that rather than a second spelling of it.
        ///
        /// Deliberately not a [`TestMeasure`](crate::text::root::test_support::TestMeasure):
        /// the probe keeps only the
        /// extent — the wrap floor and the line count are the root's — so
        /// handing one back meant inventing two of its four fields, and a
        /// test that read one was pinning the invention rather than the
        /// shaper.
        pub(crate) fn measure(&self, text: &str, shape: TestShape) -> ShapedText {
            self.probe_layout(text, shape, |probe| ShapedText {
                measured: probe.size(),
                key: probe.shaped_key(),
            })
        }

        /// Describes the fixture as a [`TextRun`] rather than lowering it
        /// straight to a request, because binding the width is now
        /// `layout`'s job — going around it here would test a path no
        /// caller takes. `TextWrap::Wrap` is the policy whose `line_fit`
        /// is the `LineFit::Wrap` this used to pass.
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
                max_width_px: shape.max_width_px,
            }))
        }

        pub(crate) fn cursor_xy(&self, text: &str, byte_offset: usize, shape: TestShape) -> Caret {
            self.probe_layout(text, shape, |probe| probe.caret_at(byte_offset))
        }

        pub(crate) fn byte_at_xy(&self, text: &str, x: f32, y: f32, shape: TestShape) -> usize {
            self.probe_layout(text, shape, |probe| probe.byte_at(x, y))
        }

        /// Whether both handles front the same shaped-buffer cache — the
        /// `HostShared` contract that a window's recorder and the backend
        /// never shape into separate caches.
        pub(crate) fn shares_cache_with(&self, other: &Self) -> bool {
            Rc::ptr_eq(&self.shared, &other.shared)
        }

        /// Hold the shaper's exclusive borrow for the caller's scope, so
        /// the backend can prove an encoded-cache hit never reaches for
        /// the shaper: anything that did would panic on the live borrow.
        ///
        /// Its one caller is the text backend's GPU suite, which stays on
        /// `internals` so a default `cargo test` needs no adapter.
        #[cfg(feature = "internals")]
        pub(crate) fn hold_borrow(&self) -> ShaperLease<'_> {
            ShaperLease {
                _inner: self.shared.inner.borrow_mut(),
            }
        }

        /// Snapshot of the shaped-buffer cache's tallies.
        pub(crate) fn cache_counts(&self) -> CacheCounts {
            self.shared.inner.borrow().cosmic.cache_counts()
        }

        /// Total cache-miss `measure` dispatches.
        pub(crate) fn measure_calls(&self) -> u64 {
            self.shared.inner.borrow().measure_calls
        }

        pub(crate) fn has_cosmic_buffer(&self, key: TextShapeKey) -> bool {
            self.shared.inner.borrow().cosmic.shaped_run(key).is_some()
        }

        /// Drop every shaped buffer now — see
        /// [`CosmicMeasure::drop_all_buffers`].
        pub(crate) fn drop_cosmic_buffers(&self) {
            self.shared.inner.borrow_mut().cosmic.drop_all_buffers();
        }
    }

    /// What the integration suites reach through `UiHarness`, and what
    /// the text benches drive.
    impl TextShaper {
        /// Deterministic mono-fallback shaper for tests and headless
        /// tools: every glyph measures `font_size_px * 0.5` wide, so a
        /// layout case states the width it expects as arithmetic rather
        /// than as whatever the bundled face happens to advance to.
        ///
        /// Over the bundled database, like any other shaper: only
        /// measurement is mono. A case that loads a font, asks which
        /// families resolve, or lays glyphs out through [`Self::glyphs`]
        /// gets the real answer.
        pub fn test_mono() -> Self {
            let shaper = Self::new();
            shaper.shared.inner.borrow_mut().mono = true;
            shaper
        }

        /// Shaped buffers currently resident.
        ///
        /// Narrower than the block: it reads through
        /// `CosmicMeasure::cache_len`, whose own module is gated to the
        /// tests and benches that ask.
        #[cfg(any(test, feature = "bench"))]
        pub(crate) fn cosmic_cache_len(&self) -> usize {
            self.shared.inner.borrow().cosmic.cache_len()
        }

        /// The lookup `TextEncoder::encode_run` performs on an
        /// encoded-cache miss: restore the shaped buffer if it aged out,
        /// and promote it onto the protected window if it is resident.
        ///
        /// Tests that model a *rendered* frame need this. Layout only
        /// ever inserts, so without the render half a buffer is never
        /// looked up and the protected window is unreachable — which is
        /// exactly the asymmetry `PROBATION_KEEP_FRAMES` documents.
        ///
        /// Narrower than the block, like [`Self::cosmic_cache_len`]: the
        /// retention tests and the text bench ask, and nothing the
        /// integration suites reach does.
        #[cfg(any(test, feature = "bench"))]
        pub(crate) fn render_ensure(&self, request: TextShapeRequest<'_>) {
            self.shared.inner.borrow_mut().cosmic.ensure_buffer(request);
        }
    }

    /// Live exclusive borrow minted by [`TextShaper::hold_borrow`].
    #[cfg(all(test, feature = "internals"))]
    #[derive(Debug)]
    pub(crate) struct ShaperLease<'a> {
        _inner: RefMut<'a, ShaperInner>,
    }
}
