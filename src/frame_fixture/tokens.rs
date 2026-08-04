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
use crate::primitives::color::Color;
use crate::primitives::corners::Corners;
use crate::primitives::shadow::Shadow;
use crate::primitives::stroke::Stroke;
use crate::scene::node::Configure;
use crate::ui::Ui;
use crate::widgets::panel::Panel;
use crate::widgets::text::Text;
use crate::widgets::theme::text_style::TextStyle;

// Surface ladder and ink. The fixture's own — the showcase runs a
// different one deliberately, so these are two designs that happen to
// both be dark, not one duplicated.
pub(super) const APP_BG: Color = Color::hex(0x0f1116);
pub(super) const CARD_BG: Color = Color::hex(0x1a1d25);
pub(super) const WELL_BG: Color = Color::hex(0x13151b);
pub(super) const BORDER: Color = Color::hex(0x2b303d);
pub(super) const TEXT_DIM: Color = Color::hex(0x8b93a7);

// Accents, aliased from the shared set under the names this tree reads
// them by: what a swatch *means* here is a threshold breach or a healthy
// delta, not "the second distinct colour".
pub(super) const ACCENT: Color = demo_swatches::TEAL;
pub(super) const WARN: Color = demo_swatches::ORANGE;
pub(super) const OK: Color = demo_swatches::LIME;
pub(super) const VIOLET: Color = demo_swatches::VIOLET;

/// Raised card: fill + hairline border + a real chrome drop shadow. The
/// shadow is the only thing in the fixture that drives the chrome branch
/// of `emit_shadow`, so keep it non-noop.
pub(super) fn card_bg() -> Background {
    Background {
        fill: CARD_BG.into(),
        stroke: Stroke::solid(BORDER, 1.0),
        corners: Corners::all(8.0),
        shadow: Shadow::drop(
            Color::rgba(0.0, 0.0, 0.0, 0.5),
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

/// Titled card: section caption over `body`, on [`card_bg`]. `h` lets a
/// card either hug its content or claim the column's leftover height
/// (the activity list is the only `FILL` one).
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
