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
//! the row stops answering through it — the width moved, the policy
//! stopped binding, or the text came to fit. Supersession has no other source — it is
//! what makes the shaped-buffer cache's probation window reachable at
//! all (see `shaped_buffer_cache::PROBATION_KEEP_FRAMES`), so a resize drag stays
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

use crate::layout::measured::Measured;
use crate::layout::text::shaped_text::ShapedText;
use crate::primitives::identity::widget_id::{WidgetId, WidgetIdSet};
use crate::primitives::layout::align::HAlign;
use crate::text::extent::TextExtent;
use crate::text::key::{TextShapeKey, WrapBound};
use crate::text::request::TextShapeRequest;
use crate::text::root::TextRoot;
use crate::text::shaper::TextShaper;
use crate::text::wrap::{TextWrap, WrapCommit, WrapFloor};
use rustc_hash::FxHashMap;
use std::mem;

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
/// [`TextShaper`]. A row lives as long as the widget that owns it: a
/// frame that touches nothing keeps every row, and [`Self::end_frame`]
/// says why that is the point rather than an oversight.
#[derive(Debug)]
pub(crate) struct TextSystem {
    shaper: TextShaper,
    /// **One widget's rows always hold the ordinals `0..k`.** They are
    /// created in record order, which numbers a widget's runs densely
    /// from zero (`TextShapeInput::on_leaf`), and they leave only as a
    /// suffix ([`Self::trim_rows`]) or whole ([`Self::end_frame`]). So a
    /// widget's rows are found by probing from ordinal 0 to the first
    /// miss, which is how both removals find them.
    entries: FxHashMap<TextRunSlot, TextReuseEntry>,
    /// Held once rather than asked per run: whether this window's shaper
    /// mints shaped buffers at all. False only under the gated mono metric.
    shapes_buffers: bool,
    /// The shaper's font epoch as of the last [`Self::sync_fonts`].
    font_epoch: u32,
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
            font_epoch: shaper.font_epoch(),
            shaper,
            entries: FxHashMap::default(),
        }
    }

    /// Drop every reuse row when the font database has moved under it,
    /// reporting whether it did — the caller owes the layout measure
    /// cache the same treatment, for the same reason.
    ///
    /// **A font load changes what a row answers without changing its
    /// key** — see [`TextShaper::font_epoch`] for why no key can carry
    /// that. So every freshness check in this file passes on a root
    /// measured in the wrong face, while the shaped buffers behind it
    /// are already gone: the render path reshapes in the new face and
    /// paints it inside a box sized for the old one.
    ///
    /// Pulled once per layout run rather than pushed by `load_font`,
    /// which is what lets `Ui::load_font` stay a `&self` call an app can
    /// make mid-record. The renderer's encoded-run cache reads the same
    /// epoch the same way, at the top of a batch
    /// (`TextEncoder::sync_fonts`).
    ///
    /// Rows are dropped rather than [superseded](TextShaper::supersede):
    /// the buffers they name no longer exist, so there is nothing left
    /// to demote.
    pub(crate) fn sync_fonts(&mut self) -> bool {
        let epoch = self.shaper.font_epoch();
        if self.font_epoch == epoch {
            return false;
        }
        self.font_epoch = epoch;
        self.entries.clear();
        true
    }

    /// Drop every row belonging to a widget that vanished.
    ///
    /// Eviction keys on the widget alone, never on a per-frame `hot`
    /// bit. The reasoning that would justify a `hot` bit — a row is only
    /// a hint, and reconstructing one costs a single refresh dispatch —
    /// holds for the *root* and fails for the
    /// wrap slot: the slot is the only record of which bounded key this
    /// row last answered, and [`Self::measure`] needs it to
    /// [`supersede`](TextShaper::supersede) that key when the row stops
    /// answering through it. Dropping it loses the demotion, and the
    /// buffer it should have demoted ages on the long window instead.
    ///
    /// That matters because the rows go cold constantly: the layout
    /// measure cache short-circuits whole subtrees, so a steadily
    /// redrawing run never touches its row at all. A sweep keyed on use
    /// would drop the slot after one still frame, and the next width
    /// change — the first frame of a drag — would have nothing to
    /// demote, once per stop-start of a jerky one.
    ///
    /// Keeping rows for live widgets bounds them by the widget's peak
    /// text-ordinal count, which is a handful per widget, and `removed`
    /// still sweeps whole widgets as they leave the tree.
    ///
    /// Named for the `FramePlan::FullRecord` frame it closes: only a
    /// frame that recorded has a `removed` set to sweep against. It does
    /// **not** advance the shared text clock, which every frame owes
    /// whether or not it recorded — `FrameRuntime::tick_text_clock` does
    /// that at the start of every frame.
    ///
    /// **Probes per removed widget rather than walking the table.** A
    /// table walk costs the map's capacity, which is the session's peak
    /// widget×ordinal count and never shrinks, so a frame that removed
    /// one widget would pay for the largest tree the UI ever showed.
    /// The prefix invariant on [`Self::entries`] is what lets the probe
    /// stop at the first missing ordinal.
    pub(crate) fn end_frame(&mut self, removed: &WidgetIdSet) {
        for &widget_id in removed {
            self.trim_rows(widget_id, 0);
        }
    }

    /// Drop the rows `widget_id` no longer records: ordinals `count` and
    /// up, where `count` is how many text runs it recorded this frame.
    ///
    /// **Called only by the pass that just measured the widget**, which
    /// is what makes it sound where a per-frame `hot` bit is not: a
    /// widget the layout measure cache short-circuited is never measured,
    /// so it never reaches here and keeps every row. What arrives is a
    /// widget that *did* record, and whose ordinals past `count` are
    /// therefore unreachable until it grows back.
    ///
    /// The rows form a prefix — see [`Self::entries`] — so the first
    /// miss ends the walk: a widget that did not shrink pays one failed
    /// lookup.
    ///
    /// **Dropped, not [retired](TextReuseEntry::retire)** — the same
    /// treatment [`Self::end_frame`] gives a widget that left the tree,
    /// and for the same reason. A list that shrank may grow back, and
    /// demoting its buffers would cost a reshape on the frame it does.
    /// Supersession is for a slot that *moved to another key*, where the
    /// old one is unreachable for good; a slot that simply stopped being
    /// recorded is the case the protected window exists for.
    pub(crate) fn trim_rows(&mut self, widget_id: WidgetId, count: u16) {
        for ordinal in count..=u16::MAX {
            if self
                .entries
                .remove(&TextRunSlot { widget_id, ordinal })
                .is_none()
            {
                return;
            }
        }
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
        debug_assert!(request.key.max_width().is_none(), "{UNBOUND_REQUEST}");
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
    ) -> RunMeasure {
        debug_assert!(request.key.max_width().is_none(), "{UNBOUND_REQUEST}");
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
            font_epoch: _,
        } = self;
        let shapes_buffers = *shapes_buffers;
        let entry = Self::refresh(entries, shaper, slot, request, wrap_policy.floor_scan());

        let Some(fit) = wrap_policy.line_fit() else {
            entry.release_bound(shaper);
            return RunMeasure {
                shaped: shaped(shapes_buffers, request.key, entry.root.extent),
                stable_from_w: 0.0,
            };
        };
        let Some(width) = available_width_px else {
            // Unbounded, a truncating fit keeps the root, as it would under
            // every width the root fits; anything else would bind to one.
            entry.release_bound(shaper);
            let stable_from_w = if fit.resolves_to_unbounded(&entry.root, f32::INFINITY) {
                entry.root.extent.size.w
            } else {
                Measured::AT_OFFER_ONLY
            };
            return RunMeasure {
                shaped: shaped(shapes_buffers, request.key, entry.root.extent),
                stable_from_w,
            };
        };
        // The same decision the probe path makes, from the same function
        // — the root is already refreshed above, so the thunk is a read.
        let bound = match wrap_policy.commit(width, halign, fit, || entry.root) {
            WrapCommit::Unbounded { extent } => {
                entry.release_bound(shaper);
                // The fit test is monotone in the width: it passes at every
                // width past this one, and at every width past the root's.
                return RunMeasure {
                    shaped: shaped(shapes_buffers, request.key, extent),
                    stable_from_w: extent.size.w.min(width),
                };
            }
            WrapCommit::Bound(bound) => bound,
        };
        let extent = if let Some(slot) = entry.wrap.filter(|slot| slot.bound == bound) {
            slot.extent
        } else {
            let extent = shaper.resolve(request.with_bound(bound));
            entry.release_bound(shaper);
            entry.wrap = Some(WrapSlot { bound, extent });
            extent
        };
        RunMeasure {
            shaped: shaped(shapes_buffers, request.key.with_bound(bound), extent),
            stable_from_w: Measured::AT_OFFER_ONLY,
        }
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
            mem::replace(entry, fresh()).retire(shaper);
        } else if floor == WrapFloor::Scan && entry.root.intrinsic_min.is_none() {
            entry.root = shaper.root(request, WrapFloor::Scan);
        }
        entry
    }
}

/// Pair an extent with the buffer key the renderer resolves it through.
/// The key is *derived* from the request rather than stored, so it cannot
/// drift from the row it came out of; the gated mono metric shapes no
/// buffer, so its runs carry `None` and the encoder drops them.
///
/// Takes `shapes_buffers` rather than `&self` because every caller is
/// mid-way through a split borrow of [`TextSystem`] and holds the flag by
/// value already.
#[inline]
fn shaped(shapes_buffers: bool, key: TextShapeKey, extent: TextExtent) -> ShapedText {
    ShapedText {
        extent,
        key: shapes_buffers.then_some(key),
    }
}

/// What [`TextSystem::measure`] answers: the run, and the committed
/// widths it answers the same under.
#[derive(Clone, Copy, Debug)]
pub(crate) struct RunMeasure {
    pub(crate) shaped: ShapedText,
    /// The least finite width from which `shaped` is the answer — zero for
    /// a policy that never binds to a width, the root's width (or the
    /// offer, if less) for a truncating fit its text fits, and
    /// [`Measured::AT_OFFER_ONLY`] for a run bound to its width, whose
    /// shape is keyed on it even where its lines fit.
    pub(crate) stable_from_w: f32,
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

impl TextReuseEntry {
    /// Give the bounded slot up, demoting the buffer it named.
    ///
    /// **The one demotion path**, and the reason it is a method rather
    /// than four spellings: a row's bounded buffer has no other
    /// reference, so every way the row stops answering through the slot
    /// — a width that moved, a policy that stopped binding, a truncating
    /// run whose text came to fit, the row's own retirement — owes the
    /// same `supersede`. One of them written without it is a buffer left
    /// on the protected window that nothing can ask for again, which is
    /// exactly the population the probation window exists to shed.
    fn release_bound(&mut self, shaper: &TextShaper) {
        if let Some(slot) = self.wrap.take() {
            shaper.supersede(self.key.with_bound(slot.bound));
        }
    }

    /// Demote every buffer this row was the last reference to: its
    /// bounded resolve, then its unbounded root.
    ///
    /// Takes the row by value, because a retired row is gone — a caller
    /// left holding one could ask it for a bound it no longer names.
    fn retire(mut self, shaper: &TextShaper) {
        self.release_bound(shaper);
        shaper.supersede(self.key);
    }
}

/// One cached width-bounded extent, under the bound that produced it.
#[derive(Clone, Copy, Debug)]
struct WrapSlot {
    bound: WrapBound,
    extent: TextExtent,
}

#[cfg(test)]
pub(crate) mod internals {
    use crate::primitives::identity::widget_id::WidgetId;
    use crate::text::request::internals::TestShape;
    use crate::text::root::internals::TestMeasure;
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
            let shaped = self.measure(slot, request, wrap_policy, shape.halign, shape.max_width);
            TestMeasure {
                size: shaped.shaped.extent.size,
                key: shaped.shaped.key,
                intrinsic_min: root.intrinsic_min,
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
