//! The fixture's design tokens and the two pieces of scaffolding every
//! section is built from.
//!
//! Colours are chosen so the console reads as a real screen on the
//! showcase's `frame bench` page, but their *only* load-bearing property
//! is that the chrome they feed stays non-noop: [`card_bg`] must keep a
//! real drop shadow (it is the sole driver of `emit_shadow`'s chrome
//! branch) and a hairline stroke, or the workload silently loses coverage.

use crate::demo_swatches;
use crate::layout::types::sizing::Sizing;
use crate::primitives::background::Background;
use crate::primitives::color::RgbaF32;
use crate::primitives::corners::Corners;
use crate::primitives::shadow::Shadow;
use crate::primitives::stroke::Stroke;
use crate::scene::node::configure::Configure;
use crate::ui::Ui;
use crate::widgets::panel::Panel;
use crate::widgets::text::Text;
use crate::widgets::theme::text_style::TextStyle;

// Surface ladder and ink. The fixture's own — the showcase runs a
// different one deliberately, so these are two designs that happen to
// both be dark, not one duplicated.
pub(super) const APP_BG: RgbaF32 = RgbaF32::hex(0x0f1116);
pub(super) const CARD_BG: RgbaF32 = RgbaF32::hex(0x1a1d25);
pub(super) const WELL_BG: RgbaF32 = RgbaF32::hex(0x13151b);
pub(super) const BORDER: RgbaF32 = RgbaF32::hex(0x2b303d);
pub(super) const TEXT_DIM: RgbaF32 = RgbaF32::hex(0x8b93a7);

// Accents, aliased from the shared set under the names this tree reads
// them by: what a swatch *means* here is a threshold breach or a healthy
// delta, not "the second distinct colour".
pub(super) const ACCENT: RgbaF32 = demo_swatches::TEAL;
pub(super) const WARN: RgbaF32 = demo_swatches::ORANGE;
pub(super) const OK: RgbaF32 = demo_swatches::LIME;
pub(super) const VIOLET: RgbaF32 = demo_swatches::VIOLET;

/// Raised card: fill + hairline border + a real chrome drop shadow. The
/// shadow is the only thing in the fixture that drives the chrome branch
/// of `emit_shadow`, so keep it non-noop.
pub(super) fn card_bg() -> Background {
    Background {
        fill: CARD_BG.into(),
        stroke: Stroke::solid(BORDER, 1.0),
        corners: Corners::all(8.0),
        shadow: Shadow::drop(
            RgbaF32::srgba(0.0, 0.0, 0.0, 0.5),
            glam::Vec2::new(0.0, 2.0),
            9.0,
        ),
    }
}

/// Recessed well — canvases and scroll strips sit on this so their
/// bounds read against the card they're inside.
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

/// Titled card: section caption over `body`, on [`card_bg`]. `h` is the
/// card's own height — `HUG` for the ones that fit their content, a
/// `Fixed` for the two that must not grow with theirs.
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
