//! Per-window text coordinator: `(WidgetId, ordinal)` reuse slots and
//! width-bounded fit resolution over the app-global shared [`TextShaper`].
//!
//! Two entry points, because layout asks two different questions.
//! [`TextSystem::root`] answers "what does this run want", and its
//! [`TextRoot`] is what `TextWrap`'s min/max-content demands are pure
//! functions of. [`TextSystem::measure`] answers "how big is it here", and
//! returns a [`ShapedText`] — an extent plus the buffer key the renderer
//! replays. Neither result carries the other's fields, so a bounded resolve
//! cannot be mistaken for a wrapping floor it never scanned for.
//!
//! These slots are a second cache in front of the shaper's own
//! content-keyed one, and **retention is what they are for**, not speed.
//!
//! A row holds the last bounded key its run answered, and that is the
//! only record of which buffer to [demote](TextShaper::supersede) when
//! the committed width moves. Supersession has no other source — it is
//! what makes the shaped-buffer cache's probation window reachable at
//! all (see `cosmic::PROBATION_KEEP_FRAMES`), so a resize drag stays
//! bounded because these rows exist. Deleting the layer would take the
//! drag bound with it, whatever a throughput benchmark says.
//!
//! **Rows are not a steady-state optimisation, because in steady state
//! they are not consulted.** The layout measure cache short-circuits
//! whole subtrees (`layout/pass.rs`), so a run that redraws unchanged
//! never reaches `TextSystem` at all. The rows earn their keep exactly
//! while something is *changing* — a drag, typing — which is also when
//! supersession matters. `text_shape/reuse_layer/*` (`src/text/bench.rs`)
//! measures 64 runs replayed straight through the layer every frame,
//! which is not a shape the engine produces; read it as an upper bound
//! on dispatch cost, not as the layer's value.

use crate::layout::ShapedText;
use crate::layout::types::align::HAlign;
use crate::primitives::size::Size;
use crate::primitives::widget_id::{WidgetId, WidgetIdSet};
use crate::text::key::{TextShapeKey, WrapBound};
use crate::text::request::TextShapeRequest;
use crate::text::root::TextRoot;
use crate::text::shaper::TextShaper;
use crate::text::wrap::{TextWrap, WrapCommit, WrapFloor};
use rustc_hash::FxHashMap;

/// Both entry points take the run's *unbounded* request and derive every
/// bounded key they need from it, so handing them a pre-bounded one would
/// silently key a row off the wrong identity. Layout only ever builds
/// unbounded requests (`TextShapeInput::shape_request`), so this is a
/// contract to assert, not a case to normalize.
///
/// Emptiness needs no such guard here: a [`TextShapeRequest`] cannot hold
/// text with no bytes, so a run with nothing to shape never reaches these
/// slots at all.
const UNBOUND_REQUEST: &str = "TextSystem entry points take an unbounded request";

/// Per-window text coordinator. Reuse slots belong to the window while
/// shaped content buffers and the font system remain shared through
/// [`TextShaper`]. Reuse rows live exactly as long as they are used:
/// every row not touched during a frame is dropped at its end.
#[derive(Debug)]
pub(crate) struct TextSystem {
    shaper: TextShaper,
    entries: FxHashMap<TextRunSlot, TextReuseEntry>,
    /// Held once rather than asked per run: whether this window's shaper
    /// mints shaped buffers at all. False only under the gated mono metric.
    shapes_buffers: bool,
}

/// Per-window reuse-slot address of one text run: the widget plus its
/// within-widget record-order ordinal select the row. A hint, not an
/// identity — [`TextSystem::measure`] validates the stored key against the
/// request, so a stale slot costs one refresh dispatch, never a wrong
/// result.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct TextRunSlot {
    pub(crate) widget_id: WidgetId,
    pub(crate) ordinal: u16,
}

impl TextSystem {
    pub(crate) fn new(shaper: TextShaper) -> Self {
        Self {
            shapes_buffers: shaper.shapes_buffers(),
            shaper,
            entries: FxHashMap::default(),
        }
    }

    /// Drop every row belonging to a widget that vanished.
    ///
    /// Eviction keys on the widget alone, never on a per-frame `hot`
    /// bit. The reasoning that would justify a `hot` bit — a row is only
    /// a hint, and reconstructing one costs a single refresh dispatch —
    /// holds for the *root* and fails for the
    /// wrap slot: the slot is the only record of which bounded key this
    /// row last answered, and [`Self::measure`] needs it to
    /// [`supersede`](TextShaper::supersede) that key when the width
    /// moves. Dropping it loses the demotion, and the buffer it should
    /// have demoted ages on the long window instead.
    ///
    /// That mattered because the rows go cold constantly: the layout
    /// measure cache short-circuits whole subtrees, so a steadily
    /// redrawing run never touches its row at all. Under the `hot` sweep
    /// a run therefore lost its slot after one still frame, and the next
    /// width change — the first frame of a drag — had nothing to demote.
    /// A jerky drag paid that once per stop-start.
    ///
    /// Keeping rows for live widgets bounds them by the widget's peak
    /// text-ordinal count, which is a handful per widget, and `removed`
    /// still sweeps whole widgets as they leave the tree.
    /// Named for the `FramePlan::FullRecord` frame it closes, because
    /// this type has two teardowns and the frame kind is what picks
    /// between them: only a frame that recorded has a `removed` set to
    /// sweep against. A `PaintOnly` frame never reaches here, and owes
    /// the clock tick below directly — `FrameCycle::run` takes it through
    /// [`TextShaper::tick_frame`](crate::TextShaper).
    ///
    /// This is where a *recorded* frame ticks the shared text frame clock
    /// — the one the renderer's glyph atlas and encoded-run cache age
    /// against too, so every text cache in the crate advances on one
    /// reading, once per window per frame. The paint-only arm named above
    /// is the other half of that "once".
    ///
    /// The empty-`removed` guard is not a micro-optimization.
    /// `HashMap::retain` walks the raw table, so it costs *capacity* —
    /// and capacity here is the session's peak widget×ordinal count,
    /// which never shrinks. Without the guard a UI that once showed a
    /// large text-bearing tree pays for that peak on every later frame
    /// to discover nothing was removed, which is the steady state:
    /// widgets leave the tree rarely, and the frames they do are already
    /// the expensive ones. Every sibling sweep in `finalize_frame`
    /// (`StateMap::sweep_removed`, `AnimMap::sweep_removed`, the
    /// `gpu_views` retain) gates the same way.
    pub(crate) fn end_frame(&mut self, removed: &WidgetIdSet) {
        // The frame's one clock tick, matching the paint-only arm in
        // `FrameCycle::run` — see `TextShaper::tick_frame` for what a
        // frozen clock does to atlas eviction.
        self.shaper.tick_frame();
        if removed.is_empty() {
            return;
        }
        self.entries
            .retain(|slot, _| !removed.contains(&slot.widget_id));
    }

    /// The run's natural shape, for the intrinsic pass. `TextWrap`'s
    /// min/max-content demands are pure functions of it.
    #[inline]
    pub(crate) fn root(
        &mut self,
        slot: TextRunSlot,
        request: TextShapeRequest<'_>,
        wrap_policy: TextWrap,
    ) -> TextRoot {
        debug_assert!(request.key.max_width_px().is_none(), "{UNBOUND_REQUEST}");
        Self::refresh(
            &mut self.entries,
            &self.shaper,
            slot,
            request,
            wrap_policy.floor_scan(),
        )
        .root
    }

    /// The run's extent at a committed width, plus the key of the shaped
    /// buffer the renderer replays. A width-bounded policy resolves its
    /// [`LineFit`](crate::text::wrap::LineFit) against the reuse root and
    /// caches the most recent bounded
    /// result in the same operation; without a width — or for policies that
    /// never bind — the root's own shape stands in.
    #[inline]
    pub(crate) fn measure(
        &mut self,
        slot: TextRunSlot,
        request: TextShapeRequest<'_>,
        wrap_policy: TextWrap,
        halign: HAlign,
        available_width_px: Option<f32>,
    ) -> ShapedText {
        debug_assert!(request.key.max_width_px().is_none(), "{UNBOUND_REQUEST}");
        if let Some(width) = available_width_px {
            debug_assert!(width.is_finite());
        }
        // Split the fields so the row stays borrowed across the shaping
        // calls below: they only ever touch `shaper`, so one lookup
        // covers both reading the row and writing its wrap slot back.
        let Self {
            shaper,
            entries,
            shapes_buffers,
        } = self;
        let shapes_buffers = *shapes_buffers;
        let entry = Self::refresh(entries, shaper, slot, request, wrap_policy.floor_scan());

        let (Some(width), Some(fit)) = (available_width_px, wrap_policy.line_fit()) else {
            return shaped(shapes_buffers, request.key, entry.root.size);
        };
        // The same decision the probe path makes, from the same function
        // — the root is already refreshed above, so the thunk is a read.
        let bound = match wrap_policy.commit(width, halign, fit, || entry.root) {
            WrapCommit::Unbounded { size } => return shaped(shapes_buffers, request.key, size),
            WrapCommit::Bound(bound) => bound,
        };
        let size = match entry.wrap.filter(|slot| slot.bound == bound) {
            Some(slot) => slot.size,
            None => {
                let size = shaper.resolve(request.with_bound(bound));
                // The width this row used to answer is now unreachable
                // through it. A resize drag leaves the *unbounded* key
                // alone and replaces only this slot, so it is the drag's
                // whole dead population — and the unbounded probe
                // `measure_truncated` re-reads every frame stays on the
                // long window, which is what keeps a drag cheap.
                if let Some(stale) = entry.wrap {
                    shaper.supersede(request.key.with_bound(stale.bound));
                }
                entry.wrap = Some(WrapSlot { bound, size });
                size
            }
        };
        shaped(shapes_buffers, request.key.with_bound(bound), size)
    }

    /// Reuse row for `slot`, reshaped if it answers a different run.
    ///
    /// `floor` is the row's own freshness axis on top of the key.
    /// The unbounded key says nothing about wrap policy, so a row filled
    /// by a policy that skipped the wrap-floor scan answers the same key
    /// as one that needs it — and would hand back a `None` floor. Asking
    /// the shaper again backfills it from the resident buffer without
    /// reshaping.
    ///
    /// Takes the two fields rather than `&mut self` so the returned row
    /// borrows only the map: a caller can then keep the row while it
    /// shapes through `shaper`, which is what lets
    /// [`Self::measure`] read a row and write its wrap slot back on one
    /// lookup.
    fn refresh<'a>(
        entries: &'a mut FxHashMap<TextRunSlot, TextReuseEntry>,
        shaper: &TextShaper,
        slot: TextRunSlot,
        request: TextShapeRequest<'_>,
        floor: WrapFloor,
    ) -> &'a mut TextReuseEntry {
        let fresh = || TextReuseEntry {
            key: request.key,
            root: shaper.root(request, floor),
            wrap: None,
        };
        let entry = entries.entry(slot).or_insert_with(&fresh);
        if entry.key != request.key {
            let stale = std::mem::replace(entry, fresh());
            retire_row(shaper, &stale);
        } else if floor == WrapFloor::Scan && entry.root.intrinsic_min.is_none() {
            entry.root = shaper.root(request, WrapFloor::Scan);
        }
        entry
    }
}

/// Pair an extent with the buffer key the renderer resolves it through.
/// The key is *derived* from the request rather than stored, so it cannot
/// drift from the row it came out of; the gated mono metric shapes no
/// buffer, so its runs carry the invalid sentinel and the encoder drops
/// them.
///
/// Takes `shapes_buffers` rather than `&self` because every caller is
/// mid-way through a split borrow of [`TextSystem`] and holds the flag by
/// value already.
#[inline]
fn shaped(shapes_buffers: bool, key: TextShapeKey, measured: Size) -> ShapedText {
    ShapedText {
        measured,
        key: if shapes_buffers {
            key
        } else {
            TextShapeKey::INVALID
        },
    }
}

/// Cached natural shape plus the most recent width-bounded resolve.
#[derive(Clone, Copy, Debug)]
struct TextReuseEntry {
    /// Unbounded request this row answers — the freshness check, and the
    /// root every bounded key it can serve is derived from.
    key: TextShapeKey,
    root: TextRoot,
    /// `None` until the row has answered a bounded width. An `Option`
    /// rather than a reserved bound: the only `max_w_q` no bound can take
    /// is the *unbounded* sentinel, so an "empty" bound re-attached
    /// through [`TextShapeKey::with_bound`] became the row's own root key
    /// — and handing that to `supersede` demotes the unbounded probe a
    /// width drag re-reads every frame, the one buffer it most needs
    /// kept. Emptiness is not a width, so it does not live in the bound.
    wrap: Option<WrapSlot>,
}

/// Demote every buffer `row` was the last reference to: each bounded
/// resolve it answered, then its unbounded root.
///
/// One place rather than one per caller. A row's reachable keys are
/// exactly its `key` plus its slots, and a caller that retires a row
/// while forgetting either half leaves those buffers ageing on the
/// protected window with nothing left that can ask for them.
fn retire_row(shaper: &TextShaper, row: &TextReuseEntry) {
    if let Some(slot) = row.wrap {
        shaper.supersede(row.key.with_bound(slot.bound));
    }
    shaper.supersede(row.key);
}

/// One cached width-bounded extent, under the bound that produced it.
#[derive(Clone, Copy, Debug)]
struct WrapSlot {
    bound: WrapBound,
    size: Size,
}

#[cfg(test)]
pub(crate) mod test_support {
    use crate::primitives::widget_id::WidgetId;
    use crate::text::request::test_support::TestShape;
    use crate::text::root::test_support::TestMeasure;
    use crate::text::shaper::TextShaper;
    use crate::text::system::{TextRunSlot, TextSystem};
    use crate::text::wrap::TextWrap;

    impl TextSystem {
        /// A system over the mono-fallback shaper — no font loading, and
        /// deterministic metrics. Named rather than `Default` because
        /// picking the mono shaper is a choice, not an absence of one.
        pub(crate) fn mono() -> Self {
            Self::new(TextShaper::test_mono())
        }

        /// A system over the real cosmic shaper — the twin of
        /// [`Self::mono`]. Callers reach the shaper back through
        /// `self.shaper` rather than holding a second handle, so a test
        /// needs no `TextShaper::new()` + `clone()` of its own.
        pub(crate) fn cosmic() -> Self {
            Self::new(TextShaper::new())
        }

        /// Both entry points against one slot, the way a frame drives them:
        /// the intrinsic pass takes the root, then the measure pass resolves
        /// a width off the row it freshened. Dispatch count is unchanged from
        /// calling [`TextSystem::measure`] alone — the root call leaves the
        /// row fresh, so the second lookup is a hit.
        pub(crate) fn shape_run(
            &mut self,
            slot: TextRunSlot,
            text: &str,
            shape: TestShape,
            wrap_policy: TextWrap,
        ) -> TestMeasure {
            let request = shape.unbounded_request(text);
            let root = self.root(slot, request, wrap_policy);
            let shaped = self.measure(slot, request, wrap_policy, shape.halign, shape.max_width_px);
            TestMeasure {
                size: shaped.measured,
                key: shaped.key,
                intrinsic_min: root.intrinsic_min,
                single_line: root.single_line,
            }
        }

        /// `true` iff a reuse row exists for `(wid, ordinal)`.
        pub(crate) fn has_entry(&self, wid: WidgetId, ordinal: u16) -> bool {
            self.entries.contains_key(&TextRunSlot {
                widget_id: wid,
                ordinal,
            })
        }

        /// Live reuse rows, for the sweep tests.
        pub(crate) fn entry_count(&self) -> usize {
            self.entries.len()
        }

        /// The shared shaper, for the reuse tests that read its counters
        /// and drive it directly. Production never reaches it from
        /// outside this file.
        pub(crate) fn shaper(&self) -> &TextShaper {
            &self.shaper
        }
    }
}
