//! The fixture's design tokens and the scaffolding every section builds from.
//!
//! The colours' only load-bearing property is that the chrome they feed stays non-noop: [`card_bg`] must keep a real drop shadow (the sole driver of `emit_shadow`'s chrome branch) and a hairline border, or the workload loses coverage.

use crate::internals::demo_swatches;
use crate::primitives::geometry::corners::Corners;
use crate::primitives::layout::sizing::Sizing;
use crate::primitives::paint::background::Background;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::shadow::Shadow;
use crate::primitives::paint::stroke::Stroke;
use crate::ui::Ui;
use crate::widget_core::configure::Configure;
use crate::widgets::panel::Panel;
use crate::widgets::text::Text;
use crate::widgets::theme::text_style::TextStyle;

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

/// Raised card: fill, hairline border and a real chrome drop shadow, the only driver of `emit_shadow`'s chrome branch; keep it non-noop.
pub(super) fn card_bg() -> Background {
    Background {
        fill: CARD_BG.into(),
        border: Stroke::new(BORDER, 1.0),
        corners: Corners::all(8.0),
        shadow: Shadow::drop(
            RgbaF32::srgba(0.0, 0.0, 0.0, 0.5),
            glam::Vec2::new(0.0, 2.0),
            9.0,
        ),
    }
}

/// Recessed well for canvases and scroll strips, so their bounds read against the card.
pub(super) fn well_bg() -> Background {
    Background {
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

/// Titled card: caption over `body` on [`card_bg`]. `h` is the card's height: `HUG` for most, `Fixed` for the two that must not grow with content.
pub(super) fn card(
    ui: &mut Ui,
    id: &'static str,
    title: &'static str,
    h: Sizing,
    body: impl FnOnce(&mut Ui),
) {
    Panel::vstack()
        .id_salt(id)
        .gap(8.0)
        .padding(10.0)
        .size((Sizing::FILL, h))
        .background(card_bg())
        .show(ui, |ui| {
            Text::new(title)
                .id_salt((id, "section"))
                .style(&section_style())
                .show(ui);
            body(ui);
        });
}
