//! The ring drawn around the keyboard-focused widget.

use crate::primitives::paint::color::RgbaF32;
use crate::theme::color_scheme::ColorScheme;
use crate::theme::metrics::Metrics;
use crate::theme::palette::Palette;

/// The ring around the focused widget, drawn by the framework, only while focus came from the keyboard (CSS `:focus-visible`).
///
/// It paints inside the widget's arranged rect with its chrome's corners, so the widget's clip never cuts it and layout never moves.
#[derive(
    Clone,
    Debug,
    PartialEq,
    serde::Serialize,
    serde::Deserialize,
    palantir_anim_derive::Lerp,
    palantir_anim_derive::TextSlots,
)]
pub struct FocusRingTheme {
    /// Ring color.
    pub color: RgbaF32,
    /// Ring width in logical px. `0` draws no ring.
    #[serde(deserialize_with = "crate::primitives::math::domain::file::length")]
    pub width: f32,
}

impl FocusRingTheme {
    /// A ring at the line step in the palette's accent.
    pub const fn from_parts(p: &Palette, m: &Metrics) -> Self {
        Self {
            color: p.accent,
            width: m.stroke.line,
        }
    }

    /// The built-in look of `scheme`: its built-in palette over [`Metrics::DEFAULT`].
    pub const fn stock(scheme: ColorScheme) -> Self {
        Self::from_parts(Palette::for_scheme(scheme), &Metrics::DEFAULT)
    }
}
