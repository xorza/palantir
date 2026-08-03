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
use crate::primitives::widget_id::WidgetId;
use crate::text::key::TextShapeKey;
use crate::text::wrap::{LineFit, TextWrap, WrapFloor};
use crate::text::{TextRoot, TextShapeRequest, TextShaper};
use rustc_hash::{FxHashMap, FxHashSet};

/// Both entry points take the run's *unbounded* request and derive every
/// bounded key they need from it, so handing them a pre-bounded one would
/// silently key a row off the wrong identity. Layout only ever builds
/// unbounded requests (`TextShapeInput::shape_request`), so this is a
/// contract to assert, not a case to normalize.
const UNBOUND_REQUEST: &str = "TextSystem entry points take an unbounded request";

/// Per-window text coordinator. Reuse slots belong to the window while
/// shaped content buffers and the font system remain shared through
/// [`TextShaper`]. Reuse rows live exactly as long as they are used:
/// every row not touched during a frame is dropped at its end.
#[derive(Debug)]
pub(crate) struct TextSystem {
    pub(super) shaper: TextShaper,
    entries: FxHashMap<(WidgetId, u16), TextReuseEntry>,
    /// Held once rather than asked per run: whether this window's shaper
    /// mints shaped buffers at all. False only under the gated mono metric.
    shapes_buffers: bool,
}

/// Per-window reuse-slot address of one text run: the widget plus its
/// within-widget record-order ordinal select the row. A hint, not an
/// identity — [`TextSystem::measure`] validates the stored key against the
/// request, so a stale slot costs one refresh dispatch, never a wrong
/// result.
#[derive(Clone, Copy, Debug)]
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
    /// Rows used to go on a per-frame `hot` bit as well, on the reasoning
    /// that a row is only a hint and reconstructing one costs a single
    /// refresh dispatch. That is true of the *root*, and false of the
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
    pub(crate) fn end_frame(&mut self, removed: &FxHashSet<WidgetId>) {
        self.shaper.end_frame();
        self.entries
            .retain(|(widget_id, _), _| !removed.contains(widget_id));
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
        if request.text.is_empty() {
            return TextRoot::ZERO;
        }
        self.refresh(slot, request, wrap_policy.floor_scan()).root
    }

    /// The run's extent at a committed width, plus the key of the shaped
    /// buffer the renderer replays. A width-bounded policy resolves its
    /// [`LineFit`] against the reuse root and caches the most recent bounded
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
        if request.text.is_empty() {
            return ShapedText {
                measured: Size::ZERO,
                key: TextShapeKey::INVALID,
            };
        }
        if let Some(width) = available_width_px {
            debug_assert!(width.is_finite());
        }
        let entry = self.refresh(slot, request, wrap_policy.floor_scan());
        let root = entry.root;
        let wrap = entry.wrap;

        let (Some(width), Some(fit)) = (available_width_px, wrap_policy.line_fit()) else {
            return self.shaped(request.key, root.size);
        };
        if fit.resolves_to_unbounded(&root, width) {
            return self.shaped(request.key, root.size);
        }
        let width = wrap_policy.target_width(width, &root);
        let slot_key = WrapSlotKey::new(width, halign, fit);
        let size = match wrap.get(slot_key) {
            Some(size) => size,
            None => {
                let size = self
                    .shaper
                    .shape_bounded(request.bounded(width, halign, fit));
                // The width this row used to answer is now unreachable
                // through it. A resize drag leaves the *unbounded* key
                // alone and replaces only this slot, so it is the drag's
                // whole dead population — and the unbounded probe
                // `measure_truncated` re-reads every frame stays on the
                // long window, which is what keeps a drag cheap.
                if let Some(stale) = wrap.key.bound(request.key) {
                    self.shaper.supersede(stale);
                }
                // Second row lookup, paid only when the committed width
                // actually moved — dwarfed by the reshape above it.
                self.refresh(slot, request, WrapFloor::Skip).wrap = WrapSlot {
                    key: slot_key,
                    size,
                };
                size
            }
        };
        self.shaped(request.key.bounded(width, halign, fit), size)
    }

    /// Reuse row for `slot`, reshaped if it answers a different run.
    ///
    /// `floor` is the row's own freshness axis on top of the key.
    /// The unbounded key says nothing about wrap policy, so a row filled
    /// by a policy that skipped the wrap-floor scan answers the same key
    /// as one that needs it — and would hand back a `None` floor. Asking
    /// the shaper again backfills it from the resident buffer without
    /// reshaping.
    fn refresh(
        &mut self,
        slot: TextRunSlot,
        request: TextShapeRequest<'_>,
        floor: WrapFloor,
    ) -> &mut TextReuseEntry {
        // Disjoint field borrows: the shaper stays readable while the map
        // is borrowed mutably. That only holds inside one body, which is
        // why callers copy what they need out of the row before reaching
        // for the shaper again.
        let shaper = &self.shaper;
        let fresh = || TextReuseEntry {
            key: request.key,
            root: shaper.shape_root(request, floor),
            wrap: WrapSlot::EMPTY,
        };
        let entry = self
            .entries
            .entry((slot.widget_id, slot.ordinal))
            .or_insert_with(&fresh);
        if entry.key != request.key {
            let stale = *entry;
            *entry = fresh();
            shaper.supersede(stale.key);
            if let Some(bounded) = stale.wrap.key.bound(stale.key) {
                shaper.supersede(bounded);
            }
        } else if floor == WrapFloor::Scan && entry.root.intrinsic_min.is_none() {
            entry.root = shaper.shape_root(request, WrapFloor::Scan);
        }
        entry
    }

    /// Pair an extent with the buffer key the renderer resolves it through.
    /// The key is *derived* from the request rather than stored, so it cannot
    /// drift from the row it came out of; the gated mono metric shapes no
    /// buffer, so its runs carry the invalid sentinel and the encoder drops
    /// them.
    #[inline]
    fn shaped(&self, key: TextShapeKey, measured: Size) -> ShapedText {
        ShapedText {
            measured,
            key: if self.shapes_buffers {
                key
            } else {
                TextShapeKey::INVALID
            },
        }
    }
}

/// Cached natural shape plus the most recent width-bounded resolve.
#[derive(Clone, Copy, Debug)]
struct TextReuseEntry {
    /// Unbounded request this row answers — the freshness check, and the
    /// root every bounded key it can serve is derived from.
    key: TextShapeKey,
    root: TextRoot,
    wrap: WrapSlot,
}

/// What distinguishes one bounded resolve of a row from another.
///
/// Six bytes rather than a second 24-byte [`TextShapeKey`]: the row's `key`
/// already pins text, size, leading, family, and weight, and
/// [`TextShapeKey::bounded`] varies nothing else.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct WrapSlotKey {
    max_w_q: u32,
    halign_q: u8,
    fit_q: u8,
}

impl WrapSlotKey {
    /// `max_w_q` no bounded key can take, so it marks an unfilled slot.
    const EMPTY: Self = Self {
        max_w_q: u32::MAX,
        halign_q: 0,
        fit_q: 0,
    };

    /// The full bounded key this slot answered, rebuilt from the row's
    /// unbounded `root`, or `None` for an unfilled slot. Six stored
    /// bytes stand in for the eighteen a second [`TextShapeKey`] would
    /// cost, and [`TextShapeKey::bounded`] varies nothing else — so
    /// re-attaching them reconstructs the key exactly.
    ///
    /// The `None` arm is load-bearing rather than defensive:
    /// [`Self::EMPTY`]'s `max_w_q` *is* the unbounded sentinel, so an
    /// unfilled slot would rebuild into the row's own root key. Handing
    /// that to `supersede` would demote the unbounded probe a width drag
    /// re-reads every frame — the one buffer the drag most needs kept.
    fn bound(self, root: TextShapeKey) -> Option<TextShapeKey> {
        (self != Self::EMPTY).then_some(TextShapeKey {
            max_w_q: self.max_w_q,
            halign_q: self.halign_q,
            fit_q: self.fit_q,
            ..root
        })
    }

    fn new(target_width_px: f32, halign: HAlign, fit: LineFit) -> Self {
        let key = TextShapeKey::INVALID.bounded(target_width_px, halign, fit);
        Self {
            max_w_q: key.max_w_q,
            halign_q: key.halign_q,
            fit_q: key.fit_q,
        }
    }
}

/// One cached width-bounded extent.
#[derive(Clone, Copy, Debug)]
struct WrapSlot {
    key: WrapSlotKey,
    size: Size,
}

impl WrapSlot {
    const EMPTY: Self = Self {
        key: WrapSlotKey::EMPTY,
        size: Size::ZERO,
    };

    fn get(self, key: WrapSlotKey) -> Option<Size> {
        (self.key == key).then_some(self.size)
    }
}

#[cfg(any(test, feature = "internals"))]
pub(crate) mod internals {
    #![allow(dead_code)]
    use crate::primitives::widget_id::WidgetId;
    use crate::text::TextShaper;
    use crate::text::internals::{TestMeasure, TestShape};
    use crate::text::system::{TextRunSlot, TextSystem};
    use crate::text::wrap::TextWrap;

    impl TextSystem {
        /// A system over the mono-fallback shaper — no font loading, and
        /// deterministic metrics. Named rather than `Default` because
        /// picking the mono shaper is a choice, not an absence of one.
        pub(crate) fn mono() -> Self {
            Self::new(TextShaper::test_mono())
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
            self.entries.contains_key(&(wid, ordinal))
        }

        /// Live reuse rows, for the sweep tests.
        pub(crate) fn entry_count(&self) -> usize {
            self.entries.len()
        }
    }
}
