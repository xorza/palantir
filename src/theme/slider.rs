//! What a slider wears: the two-tone track, and the knob that rides it.

use crate::primitives::paint::color::RgbaF32;
use crate::theme::metrics::Metrics;
use crate::theme::palette::Palette;

/// Visuals for [`crate::Slider`]: a two-tone track (`fill` left of the knob, `track` right) with a
/// round knob, in one set of colors while enabled and one while disabled.
#[derive(
    Clone,
    Debug,
    PartialEq,
    serde::Serialize,
    serde::Deserialize,
    palantir_derive::Lerp,
    palantir_derive::TextSlots,
)]
pub struct SliderTheme {
    /// The colors of a slider that takes input.
    pub enabled: SliderColors,
    /// The colors of a disabled slider, itself or by an ancestor.
    pub disabled: SliderColors,
    /// Knob diameter in logical px — also the widget's height.
    #[serde(deserialize_with = "crate::primitives::math::domain::file::length")]
    pub knob_size: f32,
    /// Track thickness in logical px; the pill radius is half.
    #[serde(deserialize_with = "crate::primitives::math::domain::file::length")]
    pub track_thickness: f32,
}

/// The three colors of a [`crate::Slider`] in one state.
#[derive(
    Clone,
    Debug,
    PartialEq,
    serde::Serialize,
    serde::Deserialize,
    palantir_derive::Lerp,
    palantir_derive::TextSlots,
)]
pub struct SliderColors {
    /// Unfilled track color (right of the knob).
    pub track: RgbaF32,
    /// Filled track color (left of the knob).
    pub fill: RgbaF32,
    /// Knob (handle) color.
    pub knob: RgbaF32,
}

impl SliderTheme {
    /// A muted track, an accent fill and a text-color knob; disabled, the fill and knob take
    /// `text_disabled` over the `element` track, as a disabled button keeps `element` with
    /// `text_disabled`, so no accent marks a control that takes no input. Its sizes are the
    /// widget's own, so it reads no metric.
    pub const fn from_parts(p: &Palette, _m: &Metrics) -> Self {
        Self {
            enabled: SliderColors {
                track: p.element_mid,
                fill: p.accent,
                knob: p.text,
            },
            disabled: SliderColors {
                track: p.element,
                fill: p.text_disabled,
                knob: p.text_disabled,
            },
            knob_size: 18.0,
            track_thickness: 4.0,
        }
    }
}
