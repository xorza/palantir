//! Per-window text coordinator: `(WidgetId, ordinal)` reuse slots and
//! width-bounded fit resolution over the app-global shared [`TextShaper`].
//! Layout consequences of a wrap policy (content/min/max sizes) are pure
//! [`TextWrap`] methods over the measurements returned here.

use crate::layout::types::align::HAlign;
use crate::primitives::widget_id::WidgetId;
use crate::text::key::TextShapeKey;
use crate::text::wrap;
use crate::text::wrap::{LineFit, TextWrap};
use crate::text::{TextMeasurement, TextShapeRequest, TextShaper};
use rustc_hash::{FxHashMap, FxHashSet};

/// Per-window text coordinator. Reuse slots belong to the window while
/// shaped content buffers and the font system remain shared through
/// [`TextShaper`]. Reuse rows are clock-swept under size pressure.
#[derive(Debug)]
pub(crate) struct TextSystem {
    pub(crate) shaper: TextShaper,
    pub(crate) entries: FxHashMap<(WidgetId, u16), TextReuseEntry>,
    pub(crate) sweep_limit: usize,
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

const MIN_REUSE_SWEEP_LIMIT: usize = 256;

impl TextSystem {
    pub(crate) fn new(shaper: TextShaper) -> Self {
        Self {
            shaper,
            entries: FxHashMap::default(),
            sweep_limit: MIN_REUSE_SWEEP_LIMIT,
        }
    }

    pub(crate) fn end_frame(&mut self, removed: &FxHashSet<WidgetId>) {
        self.shaper.end_frame();
        let previous_len = self.entries.len();
        let sweep = previous_len > self.sweep_limit;
        if !sweep && removed.is_empty() {
            return;
        }
        self.entries.retain(|(widget_id, _), entry| {
            !removed.contains(widget_id) && (!sweep || std::mem::take(&mut entry.hot))
        });
        if sweep || self.entries.len() != previous_len {
            self.sweep_limit = next_reuse_sweep_limit(self.entries.len());
        }
    }

    /// Measure one slot-cached text run under `wrap_policy`. The unbounded
    /// measurement is the reuse root; a committed width resolves the
    /// policy's [`LineFit`] against it and caches the most recent bounded
    /// measurement in the same operation. Without an available width (the
    /// intrinsic path) — or for policies that never bind — the unbounded
    /// root is returned directly. Content/min/max layout consequences are
    /// pure [`TextWrap`] methods over the returned measurement.
    #[inline]
    pub(crate) fn measure(
        &mut self,
        slot: TextRunSlot,
        request: TextShapeRequest<'_>,
        wrap_policy: TextWrap,
        halign: HAlign,
        available_width_px: Option<f32>,
    ) -> TextMeasurement {
        let shaper = &self.shaper;
        let request = request.unbounded_version();
        if request.text.is_empty() {
            return TextMeasurement::ZERO;
        }

        let refresh = || TextReuseEntry {
            key: request.key,
            unbounded: shaper.inner.borrow_mut().dispatch(request),
            wrap: None,
            hot: true,
        };
        let entry = self
            .entries
            .entry((slot.widget_id, slot.ordinal))
            .or_insert_with(&refresh);
        if entry.key != request.key {
            *entry = refresh();
        } else {
            entry.hot = true;
        }
        if let Some(width) = available_width_px {
            debug_assert!(width.is_finite());
        }
        let unbounded = entry.unbounded;
        let (Some(width), Some(fit)) = (available_width_px, wrap_policy.line_fit()) else {
            return unbounded;
        };
        // A fitting single-line Clip/Ellipsis resolve shapes glyphs
        // identical to the unbounded root (truncated shaping is
        // halign-independent and single-line by construction), so the root
        // stands in — no second shape, no second cache entry. Not valid for
        // `Wrap` fits: cosmic bakes per-line halign offsets into wrapped
        // buffers. `size.w` is ceil'd and the canonical width is integral,
        // so this fit check matches the truncating path's exactly.
        if matches!(fit, LineFit::Clip | LineFit::Ellipsis)
            && unbounded.single_line
            && unbounded.size.w <= wrap::canonical_wrap_width(width)
        {
            return unbounded;
        }
        let width = match wrap_policy {
            // Words wider than the committed width overflow rather than break.
            TextWrap::WrapWithOverflow => width.max(unbounded.intrinsic_min),
            _ => width,
        };
        resolve_bounded_measurement(shaper, entry, request, width, halign, fit)
    }
}

fn resolve_bounded_measurement(
    shaper: &TextShaper,
    entry: &mut TextReuseEntry,
    request: TextShapeRequest<'_>,
    target_width_px: f32,
    halign: HAlign,
    fit: LineFit,
) -> TextMeasurement {
    let request = request.bounded(target_width_px, halign, fit);
    if let Some(wrap) = entry.wrap
        && wrap.key == request.key
    {
        return wrap.result;
    }
    let measurement = shaper.inner.borrow_mut().dispatch(request);
    entry.wrap = Some(WrapReuse {
        key: request.key,
        result: measurement,
    });
    measurement
}

/// Cached unbounded shape + most-recent wrap result.
#[derive(Clone, Copy, Debug)]
pub(crate) struct TextReuseEntry {
    key: TextShapeKey,
    unbounded: TextMeasurement,
    wrap: Option<WrapReuse>,
    hot: bool,
}

pub(crate) fn next_reuse_sweep_limit(len: usize) -> usize {
    len.saturating_add(1)
        .checked_next_power_of_two()
        .unwrap_or(usize::MAX)
        .max(MIN_REUSE_SWEEP_LIMIT)
}

/// One cached width-bounded result.
#[derive(Clone, Copy, Debug)]
struct WrapReuse {
    key: TextShapeKey,
    result: TextMeasurement,
}

#[cfg(any(test, feature = "internals"))]
pub(crate) mod test_support {
    #![allow(dead_code)]
    use crate::primitives::widget_id::WidgetId;
    use crate::text::TextMeasurement;
    use crate::text::TextShaper;
    use crate::text::system::{TextRunSlot, TextSystem};
    use crate::text::test_support::TestShape;
    use crate::text::wrap::TextWrap;

    pub(crate) trait TextSystemTestExt {
        fn shape_run(
            &mut self,
            slot: TextRunSlot,
            text: &str,
            shape: TestShape,
            wrap_policy: TextWrap,
        ) -> TextMeasurement;
    }

    impl TextSystemTestExt for TextSystem {
        fn shape_run(
            &mut self,
            slot: TextRunSlot,
            text: &str,
            shape: TestShape,
            wrap_policy: TextWrap,
        ) -> TextMeasurement {
            TextSystem::measure(
                self,
                slot,
                shape.unbounded_request(text),
                wrap_policy,
                shape.halign,
                shape.max_width_px,
            )
        }
    }

    impl Default for TextSystem {
        fn default() -> Self {
            Self::new(TextShaper::test_mono())
        }
    }

    impl TextSystem {
        /// `true` iff a reuse row exists for `(wid, ordinal)`.
        pub(crate) fn has_entry(&self, wid: WidgetId, ordinal: u16) -> bool {
            self.entries.contains_key(&(wid, ordinal))
        }
    }
}
