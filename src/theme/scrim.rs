//! [`Scrim`]: what a modal lays over the window behind its panel.

use crate::primitives::math::domain;
use crate::primitives::paint::backdrop_filter::BackdropFilter;
use crate::primitives::paint::chrome::Chrome;
use crate::primitives::paint::color::RgbaF32;

/// What a [`Modal`](crate::Modal) lays over the window behind its panel: a tint, over the window
/// seen through a backdrop filter. With [`BackdropFilter::NONE`], the stock look, it only dims,
/// as Fluent's ContentDialog smoke layer, Material 3's scrim, Apple's alerts and libadwaita's
/// `AdwDialog` do; a filter blurs the window too, as CSS's `::backdrop` takes a
/// `backdrop-filter`.
///
/// A filter costs a capture and a blur of the whole window on each frame where something under
/// the scrim changes: 394 µs and 732 µs of device time at 3840 × 2160 on a mobile AMD iGPU
/// (`integration/backdrop_frame/gpu/modal`, `.notes/research/2026-10-28-backdrops.md`), and
/// nothing on a frame where nothing does.
///
/// Under reduced transparency the scrim drops its filter and keeps its tint, to the sRGB byte a
/// filter's fallback stores: the filter's own fallback is not read, as a pane's fill made opaque
/// would black out the window behind a half-clear tint. [`From<RgbaF32>`] makes a scrim that only dims.
#[derive(
    Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize, palantir_derive::Lerp,
)]
#[must_use]
pub struct Scrim {
    /// The tint; straight-alpha linear.
    pub tint: RgbaF32,
    /// What the scrim does to the window under its tint; [`BackdropFilter::NONE`] (the
    /// `Default`) leaves it and is omitted from serialized output.
    #[serde(default, skip_serializing_if = "BackdropFilter::is_noop")]
    pub filter: BackdropFilter,
}

impl Scrim {
    /// This scrim over the window seen through `filter`.
    pub const fn with_filter(mut self, filter: BackdropFilter) -> Self {
        self.filter = filter;
        self
    }

    /// Panics unless the tint is a [color](crate::widget::domain::color) and the filter passes
    /// [`BackdropFilter`]'s checks.
    #[track_caller]
    pub(crate) const fn validate(&self) {
        let _ = domain::color(self.tint);
        self.filter.validate();
    }

    /// The chrome the modal's root paints: the tint over the filter, whose fallback is the tint,
    /// so reduced transparency dims and does not black out.
    pub(crate) fn chrome(&self) -> Chrome {
        Chrome::fill(self.tint).with_backdrop_filter(self.filter.with_fallback(self.tint))
    }
}

impl From<RgbaF32> for Scrim {
    /// A scrim that only dims, in `tint`.
    fn from(tint: RgbaF32) -> Self {
        Self {
            tint,
            filter: BackdropFilter::NONE,
        }
    }
}
