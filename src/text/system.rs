//! Per-window text coordinator: `(WidgetId, ordinal)` identity reuse and
//! wrap-policy resolution over the app-global shared [`TextShaper`].

use crate::layout::types::align::HAlign;
use crate::primitives::size::Size;
use crate::primitives::widget_id::WidgetId;
use crate::text::key::{LineFit, TextShapeKey};
use crate::text::wrap;
use crate::text::wrap::TextWrap;
use crate::text::{TextMeasurement, TextShapeRequest, TextShaper};
use rustc_hash::{FxHashMap, FxHashSet};
use std::collections::hash_map::Entry;

/// Per-window text coordinator. Identity reuse belongs to the window while
/// shaped content buffers and the font system remain shared through
/// [`TextShaper`]. Reuse rows are clock-swept under size pressure.
#[derive(Debug)]
pub(crate) struct TextSystem {
    pub(crate) shaper: TextShaper,
    pub(crate) entries: FxHashMap<(WidgetId, u16), TextReuseEntry>,
    pub(crate) sweep_limit: usize,
}

/// Shaped-buffer measurement plus every layout consequence of one
/// [`TextWrap`] policy.
#[derive(Clone, Copy, Debug)]
pub(crate) struct TextShapeResult {
    pub(crate) measurement: TextMeasurement,
    pub(crate) content_size: Size,
    pub(crate) min_content: Size,
    pub(crate) max_content: Size,
}

impl TextShapeResult {
    const ZERO: Self = Self {
        measurement: TextMeasurement::ZERO,
        content_size: Size::ZERO,
        min_content: Size::ZERO,
        max_content: Size::ZERO,
    };
}

/// Per-window identity of one text run. The widget and ordinal select its
/// reuse row; [`TextSystem`] derives validity from the shaping inputs.
#[derive(Clone, Copy, Debug)]
pub(crate) struct TextRunIdentity {
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
        if previous_len > self.sweep_limit {
            if removed.is_empty() {
                self.entries
                    .retain(|_, entry| std::mem::take(&mut entry.hot));
            } else {
                self.entries.retain(|(widget_id, _), entry| {
                    !removed.contains(widget_id) && std::mem::take(&mut entry.hot)
                });
            }
            self.sweep_limit = next_reuse_sweep_limit(self.entries.len());
            return;
        }
        if removed.is_empty() {
            return;
        }
        self.entries
            .retain(|(widget_id, _), _| !removed.contains(widget_id));
        if self.entries.len() != previous_len {
            self.sweep_limit = next_reuse_sweep_limit(self.entries.len());
        }
    }

    /// Shape one identity-cached text run. The unbounded measurement remains
    /// the reuse root; bounded policies derive their target from it and cache
    /// the most recent resolved measurement in the same operation. Inlining
    /// lets each hot caller erase the result fields it does not consume.
    #[inline]
    pub(crate) fn shape(
        &mut self,
        identity: TextRunIdentity,
        request: TextShapeRequest<'_>,
        wrap_policy: TextWrap,
        halign: HAlign,
        available_width_px: Option<f32>,
    ) -> TextShapeResult {
        let shaper = &self.shaper;
        let request = request.unbounded_version();
        if request.text.is_empty() {
            return TextShapeResult::ZERO;
        }

        let refresh = || {
            let unbounded = shaper.inner.borrow_mut().dispatch(request);
            TextReuseEntry {
                key: request.key,
                unbounded,
                wrap: None,
                hot: true,
            }
        };
        let entry = match self.entries.entry((identity.widget_id, identity.ordinal)) {
            Entry::Occupied(mut occupied) => {
                if occupied.get().key != request.key {
                    occupied.insert(refresh());
                } else {
                    occupied.get_mut().hot = true;
                }
                occupied.into_mut()
            }
            Entry::Vacant(vacant) => vacant.insert(refresh()),
        };
        if let Some(width) = available_width_px {
            debug_assert!(width.is_finite());
        }
        let unbounded = entry.unbounded;
        let zero_width = Size::new(0.0, unbounded.size.h);
        match wrap_policy {
            TextWrap::SingleLine => TextShapeResult {
                measurement: unbounded,
                content_size: unbounded.size,
                min_content: unbounded.size,
                max_content: unbounded.size,
            },
            // Scroll owns clipping and panning, so its full run creates no width demand.
            TextWrap::Scroll => TextShapeResult {
                measurement: unbounded,
                content_size: zero_width,
                min_content: zero_width,
                max_content: zero_width,
            },
            TextWrap::Truncate => {
                let measurement = available_width_px.map_or(unbounded, |width| {
                    resolve_bounded_measurement(
                        shaper,
                        entry,
                        request,
                        width,
                        halign,
                        LineFit::Clip,
                    )
                });
                TextShapeResult {
                    measurement,
                    content_size: measurement.size,
                    min_content: zero_width,
                    max_content: unbounded.size,
                }
            }
            TextWrap::Ellipsis => {
                let measurement = available_width_px.map_or(unbounded, |width| {
                    resolve_bounded_measurement(
                        shaper,
                        entry,
                        request,
                        width,
                        halign,
                        LineFit::Ellipsis,
                    )
                });
                TextShapeResult {
                    measurement,
                    content_size: measurement.size,
                    min_content: zero_width,
                    max_content: unbounded.size,
                }
            }
            TextWrap::Wrap => {
                let measurement = available_width_px.map_or(unbounded, |width| {
                    resolve_bounded_measurement(
                        shaper,
                        entry,
                        request,
                        width,
                        halign,
                        LineFit::Wrap,
                    )
                });
                TextShapeResult {
                    measurement,
                    content_size: measurement.size,
                    min_content: zero_width,
                    max_content: unbounded.size,
                }
            }
            TextWrap::WrapWithOverflow => {
                let measurement = available_width_px.map_or(unbounded, |width| {
                    resolve_bounded_measurement(
                        shaper,
                        entry,
                        request,
                        width.max(unbounded.intrinsic_min),
                        halign,
                        LineFit::Wrap,
                    )
                });
                TextShapeResult {
                    measurement,
                    content_size: measurement.size,
                    min_content: Size::new(unbounded.intrinsic_min, unbounded.size.h),
                    max_content: unbounded.size,
                }
            }
        }
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
    let target_width_px = wrap::canonical_wrap_width(target_width_px);
    let request = request
        .bounded(target_width_px, halign, fit)
        .expect("canonical text wrap width must be valid");
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
    use crate::text::system::{TextRunIdentity, TextSystem};
    use crate::text::test_support::TestShape;
    use crate::text::wrap::TextWrap;

    pub(crate) trait TextSystemTestExt {
        fn shape_run(
            &mut self,
            identity: TextRunIdentity,
            text: &str,
            shape: TestShape,
            wrap_policy: TextWrap,
        ) -> Option<TextMeasurement>;
    }

    impl TextSystemTestExt for TextSystem {
        fn shape_run(
            &mut self,
            identity: TextRunIdentity,
            text: &str,
            shape: TestShape,
            wrap_policy: TextWrap,
        ) -> Option<TextMeasurement> {
            shape.unbounded_request(text).map(|request| {
                TextSystem::shape(
                    self,
                    identity,
                    request,
                    wrap_policy,
                    shape.halign,
                    shape.max_width_px,
                )
                .measurement
            })
        }
    }

    impl Default for TextSystem {
        fn default() -> Self {
            Self::new(TextShaper::default())
        }
    }

    impl TextSystem {
        /// `true` iff an identity row exists for `(wid, ordinal)`.
        pub(crate) fn has_entry(&self, wid: WidgetId, ordinal: u16) -> bool {
            self.entries.contains_key(&(wid, ordinal))
        }
    }
}
