//! The fixture's design tokens: colors, chrome and text styles every section builds from.
//!
//! The colors' only load-bearing property is that the chrome they feed stays non-noop: [`card_chrome`] must keep a real drop shadow (the sole driver of the encoder's chrome-shadow branch) and a hairline border, or the workload loses coverage.

use crate::internals::demo_swatches;
use crate::primitives::geometry::corners::Corners;
use crate::primitives::paint::chrome::Chrome;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::shadow::Shadow;
use crate::primitives::paint::stroke::Stroke;
use crate::theme::text_style::TextStyle;
use glam::Vec2;

// Surface ladder and ink: the fixture's own; the showcase runs a different dark one deliberately.
pub(super) const APP_BG: RgbaF32 = RgbaF32::hex(0x0f1116);
pub(super) const CARD_BG: RgbaF32 = RgbaF32::hex(0x1a1d25);
pub(super) const WELL_BG: RgbaF32 = RgbaF32::hex(0x13151b);
pub(super) const BORDER: RgbaF32 = RgbaF32::hex(0x2b303d);
pub(super) const TEXT_DIM: RgbaF32 = RgbaF32::hex(0x8b93a7);

// Accents aliased from the shared set under this tree's meaning: a threshold breach or healthy delta.
pub(super) const ACCENT: RgbaF32 = demo_swatches::TEAL;
pub(super) const WARN: RgbaF32 = demo_swatches::ORANGE;
pub(super) const OK: RgbaF32 = demo_swatches::LIME;
pub(super) const VIOLET: RgbaF32 = demo_swatches::VIOLET;

/// Raised card: fill, hairline border and a real chrome drop shadow, the only driver of the encoder's chrome-shadow branch; keep it non-noop.
pub(super) fn card_chrome() -> Chrome {
    Chrome {
        fill: CARD_BG.into(),
        border: Stroke::new(BORDER, 1.0),
        corners: Corners::all(8.0),
        shadow: Shadow::drop(RgbaF32::srgba(0.0, 0.0, 0.0, 0.5), Vec2::new(0.0, 2.0), 9.0),
    }
}

/// Recessed well for canvases and scroll strips, so their bounds read against the card.
pub(super) fn well_chrome() -> Chrome {
    Chrome {
        fill: WELL_BG.into(),
        corners: Corners::all(6.0),
        ..Default::default()
    }
}

pub(super) fn section_style() -> TextStyle {
    TextStyle::default()
        .with_font_size(11.0)
        .with_color(TEXT_DIM)
        .bold()
}

pub(super) fn caption_style() -> TextStyle {
    TextStyle::default()
        .with_font_size(11.0)
        .with_color(TEXT_DIM)
}

pub(super) fn body_style() -> TextStyle {
    TextStyle::default().with_font_size(13.0)
}
