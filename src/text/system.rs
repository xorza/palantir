//! Per-window text coordinator: `(WidgetId, ordinal)` reuse slots and
//! width-bounded fit resolution over the shared [`TextShaper`].
//!
//! [`TextSystem::root`] answers "what does this run want" ([`TextRoot`], from
//! which `TextWrap`'s min/max-content demands follow); [`TextSystem::measure`]
//! answers "how big is it here" ([`RunMeasure`] holding a [`ShapedText`]).
//! Neither carries the other's fields, so a bounded resolve can't pass for a
//! wrapping floor it never scanned for.
//!
//! The slots are a second cache in front of the shaper's content-keyed one, for
//! retention, not speed. A row holds the last bounded key its run answered, the
//! only record of which buffer to [demote](TextShaper::supersede) when the row
//! stops answering through it. Supersession makes the shaped-buffer cache's
//! probation window reachable (`shaped_buffer_cache::PROBATION_KEEP_FRAMES`) and
//! so bounds a resize drag. In steady state rows go unconsulted (the layout
//! measure cache skips whole subtrees); the `text_shape/reuse_layer/*` bench
//! (`src/text/bench.rs`) replays runs every frame, so read it as an upper bound.

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

/// Both entry points take the run's unbounded request and derive every bounded key
/// from it; a pre-bounded one would key a row off the wrong identity, so it is
/// asserted. Empty text never reaches these slots ([`TextShapeRequest`] can't hold it).
const UNBOUND_REQUEST: &str = "TextSystem entry points take an unbounded request";

/// Per-window text coordinator. Reuse slots belong to the window; shaped buffers
/// and the font system are shared through [`TextShaper`].
#[derive(Debug)]
pub(crate) struct TextSystem {
    shaper: TextShaper,
    /// One widget's rows always hold ordinals `0..k` (created in record order, removed
    /// only as a suffix or whole), so probing from 0 to the first miss finds them.
    entries: FxHashMap<TextRunSlot, TextReuseEntry>,
    /// Whether this window's shaper mints shaped buffers; false only under mono.
    shapes_buffers: bool,
    font_epoch: u32,
}

/// Per-window reuse-slot address of one text run. A hint, not an identity:
/// [`TextSystem::measure`] validates the stored key, so a stale slot costs one
/// refresh.
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

    /// Drop every reuse row when the font database has moved, reporting whether it
    /// did; the caller owes the layout measure cache the same. A font load changes a
    /// row's answer without changing its key (see [`TextShaper::font_epoch`]), so
    /// freshness checks would pass on a root measured in the wrong face. Pulled once
    /// per layout run so `Ui::load_font` stays `&self`. Rows are dropped, not
    /// [superseded](TextShaper::supersede): their buffers are gone.
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
    /// Keyed on the widget alone, never a per-frame `hot` bit: the wrap slot is the
    /// only record of which bounded key the row last answered, which [`Self::measure`]
    /// needs to [`supersede`](TextShaper::supersede) it. Rows go cold constantly
    /// (the measure cache skips steady subtrees), so a use-keyed sweep would leave
    /// the first frame of a drag nothing to demote.
    ///
    /// Closes the `FramePlan::FullRecord` frame, the only kind with a `removed` set.
    /// Does not advance the shared text clock (`FrameRuntime::tick_text_clock` does).
    /// Probes per removed widget instead of walking the table, whose cost is the
    /// session's peak and never shrinks.
    pub(crate) fn end_frame(&mut self, removed: &WidgetIdSet) {
        for &widget_id in removed {
            self.trim_rows(widget_id, 0);
        }
    }

    /// Drop the rows `widget_id` no longer records: ordinals `count` and up. Called
    /// only by the pass that just measured the widget; one the measure cache
    /// short-circuited keeps every row. Dropped, not [retired](TextReuseEntry::retire):
    /// a list that shrank may grow back, and demoting its buffers would cost a reshape.
    /// Supersession is for a slot that moved to another key.
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

    /// The run's extent at a committed width plus the key of the shaped buffer the
    /// renderer replays. A width-bounded policy resolves its
    /// [`LineFit`](crate::text::wrap::LineFit) against the reuse root and caches the
    /// result; otherwise the root's shape stands in.
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
        // Split the fields so the row stays borrowed across the shaping calls.
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
            // Unbounded, a truncating fit keeps the root, as under every width it fits.
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
        // The same decision as the probe path; the root is already refreshed.
        let bound = match wrap_policy.commit(width, halign, fit, || entry.root) {
            WrapCommit::Unbounded { extent } => {
                entry.release_bound(shaper);
                // The fit test is monotone in width.
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

    /// Reuse row for `slot`, reshaped if it answers a different run. `floor` is a
    /// freshness axis beyond the key: a row filled by a policy that skipped the floor
    /// scan would return a `None` floor, so the shaper is asked again (backfilling
    /// from the resident buffer, no reshape). Takes the two fields, not `&mut self`,
    /// so the row borrows only the map.
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

/// Pair an extent with the buffer key the renderer resolves it through. The key
/// is derived from the request so it can't drift; mono shapes no buffer, so it
/// carries `None`.
#[inline]
fn shaped(shapes_buffers: bool, key: TextShapeKey, extent: TextExtent) -> ShapedText {
    ShapedText {
        extent,
        key: shapes_buffers.then_some(key),
    }
}

/// What [`TextSystem::measure`] answers: the run and the widths it answers the
/// same under.
#[derive(Clone, Copy, Debug)]
pub(crate) struct RunMeasure {
    pub(crate) shaped: ShapedText,
    /// The least finite width from which `shaped` is the answer: zero for a policy that
    /// never binds, the root's width (or the offer, if less) for a truncating fit its
    /// text fits, [`Measured::AT_OFFER_ONLY`] for a run bound to its width.
    pub(crate) stable_from_w: f32,
}

#[derive(Clone, Copy, Debug)]
struct TextReuseEntry {
    key: TextShapeKey,
    root: TextRoot,
    /// `None` until the row has answered a bounded width. An `Option`, not a reserved
    /// bound: an "empty" bound re-attached via [`TextShapeKey::with_bound`] becomes the
    /// row's root key, and `supersede` would demote the unbounded probe a drag re-reads
    /// every frame.
    wrap: Option<WrapSlot>,
}

impl TextReuseEntry {
    /// Give the bounded slot up, demoting its buffer. The one demotion path: that
    /// buffer has no other reference, so every way a row stops answering through the
    /// slot owes the same `supersede`, or it is stranded on the protected window.
    fn release_bound(&mut self, shaper: &TextShaper) {
        if let Some(slot) = self.wrap.take() {
            shaper.supersede(self.key.with_bound(slot.bound));
        }
    }

    /// Demote every buffer this row was the last reference to. Takes the row by
    /// value because a retired row is gone.
    fn retire(mut self, shaper: &TextShaper) {
        self.release_bound(shaper);
        shaper.supersede(self.key);
    }
}

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
        /// A system over the mono-fallback shaper. Named, not `Default`, because mono is
        /// a choice.
        pub(crate) fn mono() -> Self {
            Self::new(TextShaper::test_mono())
        }

        /// A system over the real cosmic shaper, twin of [`Self::mono`].
        pub(crate) fn cosmic() -> Self {
            Self::new(TextShaper::new())
        }

        /// Both entry points against one slot, as a frame drives them; the root call
        /// leaves the row fresh, so dispatch count equals [`TextSystem::measure`] alone.
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

        pub(crate) fn has_entry(&self, wid: WidgetId, ordinal: u16) -> bool {
            self.entries.contains_key(&TextRunSlot {
                widget_id: wid,
                ordinal,
            })
        }

        pub(crate) fn entry_count(&self) -> usize {
            self.entries.len()
        }

        pub(crate) fn shaper(&self) -> &TextShaper {
            &self.shaper
        }
    }
}
