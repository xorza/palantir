//! Shared scaffolding for showcase pages: the four-color "layout
//! swatch" palette plus the page/section/cell helpers every page
//! builds from, so all tabs share one set of visual conventions.
//!
//! Conventions: pages add no root padding (the shell's central card
//! already pads 16), captions are 12 px, demo surfaces use
//! [`panel_bg`] / [`surface_bg`], and swatch rectangles pull from the
//! `A`–`D` palette — picked from the Ayu syntax-color block so they
//! harmonize with the default theme.

use aperture::{
    Background, Color, Configure, Corners, Panel, Sizing, Stroke, Text, TextStyle, TextWrap, Ui,
};
use std::hash::Hash;

/// Teal-blue. Default swatch when one color is enough.
pub(crate) const A: Color = Color::hex(0x4cd3ff);
/// Orange. Pair with `A` for "two distinct things".
pub(crate) const B: Color = Color::hex(0xffa63d);
/// Green.
pub(crate) const C: Color = Color::hex(0xd9ff57);
/// Purple.
pub(crate) const D: Color = Color::hex(0xd897ff);

/// Standard swatch fill — colored rect with a 4 px corner radius.
pub(crate) fn swatch_bg(c: Color) -> Background {
    Background {
        fill: c.into(),
        corners: Corners::all(4.0),
        ..Default::default()
    }
}

/// Recessed demo-surface fill, one shade darker than the showcase card
/// (`#343434`) so a demo's bounds read against the surrounding card.
pub(crate) fn panel_bg() -> Background {
    Background {
        fill: Color::hex(0x252525).into(),
        corners: Corners::all(4.0),
        ..Default::default()
    }
}

/// Light demo surface — for content that only reads against a light
/// backdrop (drop shadows, dark strokes).
pub(crate) fn light_panel_bg() -> Background {
    Background {
        fill: Color::hex(0xc8ccd2).into(),
        corners: Corners::all(4.0),
        ..Default::default()
    }
}

/// Raised interactive surface: dark fill + hairline stroke. Used for
/// popup bodies, right-click targets, and similar chrome-like demos.
pub(crate) fn surface_bg() -> Background {
    Background {
        fill: Color::hex(0x2a2a2a).into(),
        stroke: Stroke::solid(Color::hex(0x4a4a4a), 1.0),
        corners: Corners::all(6.0),
        ..Default::default()
    }
}

/// 12 px caption, default color. Used for section titles and small labels.
pub(crate) fn caption_style() -> TextStyle {
    TextStyle::default().with_font_size(12.0)
}

/// Near-black text for placing on top of a bright swatch fill — a
/// legibility requirement, not decoration.
pub(crate) fn on_swatch_text() -> TextStyle {
    TextStyle::default()
        .with_font_size(13.0)
        .with_color(Color::hex(0x1a1a1a))
}

/// Page root: fills the shell's central card. No padding — the card
/// already pads 16.
pub(crate) fn page(ui: &mut Ui, body: impl FnOnce(&mut Ui)) {
    Panel::vstack()
        .auto_id()
        .gap(16.0)
        .size((Sizing::FILL, Sizing::FILL))
        .show(ui, body);
}

/// One-line page description under the tab bar — 13 px, wraps.
pub(crate) fn header(ui: &mut Ui, text: &'static str) {
    Text::new(text)
        .id_salt(("page-header", text))
        .style(TextStyle::default().with_font_size(13.0))
        .text_wrap(TextWrap::WrapWithOverflow)
        .show(ui);
}

/// Title + body pair: a small caption above a child block. No card
/// decoration — the surrounding showcase panel already contains the demo.
pub(crate) fn section<H: Hash + Copy>(
    ui: &mut Ui,
    id: H,
    title: &'static str,
    body: impl FnOnce(&mut Ui),
) {
    Panel::vstack()
        .id_salt(id)
        .size((Sizing::FILL, Sizing::Hug))
        .gap(6.0)
        .show(ui, |ui| {
            Text::new(title)
                .id_salt((id, "section-title"))
                .style(caption_style())
                .show(ui);
            body(ui);
        });
}

/// Horizontal row of controls, hugging its content height.
pub(crate) fn row<H: Hash + Copy>(ui: &mut Ui, id: H, body: impl FnOnce(&mut Ui)) {
    Panel::hstack()
        .id_salt(id)
        .size((Sizing::FILL, Sizing::Hug))
        .gap(8.0)
        .show(ui, body);
}

/// Row of [`demo_cell`]s sharing the parent's leftover height equally.
pub(crate) fn cell_row(ui: &mut Ui, id: &'static str, body: impl FnOnce(&mut Ui)) {
    Panel::hstack()
        .id_salt(id)
        .size((Sizing::FILL, Sizing::FILL))
        .gap(12.0)
        .show(ui, body);
}

/// Captioned demo cell: label above a recessed FILL surface the body
/// paints into. The caption makes paint demos self-describing.
pub(crate) fn demo_cell(ui: &mut Ui, label: &'static str, body: impl FnOnce(&mut Ui)) {
    demo_cell_on(ui, label, panel_bg(), body);
}

/// [`demo_cell`] on the light surface — for shadow/dark-stroke content.
pub(crate) fn demo_cell_light(ui: &mut Ui, label: &'static str, body: impl FnOnce(&mut Ui)) {
    demo_cell_on(ui, label, light_panel_bg(), body);
}

/// Caption above a bare FILL body — for demos that paint their own
/// surface (clip cards, gradients) where a recessed bg would double up.
pub(crate) fn captioned_cell(ui: &mut Ui, label: &'static str, body: impl FnOnce(&mut Ui)) {
    Panel::vstack()
        .id_salt(label)
        .size((Sizing::FILL, Sizing::FILL))
        .gap(4.0)
        .show(ui, |ui| {
            Text::new(label)
                .id_salt((label, "cell-caption"))
                .style(caption_style())
                .show(ui);
            body(ui);
        });
}

fn demo_cell_on(ui: &mut Ui, label: &'static str, bg: Background, body: impl FnOnce(&mut Ui)) {
    Panel::vstack()
        .id_salt(label)
        .size((Sizing::FILL, Sizing::FILL))
        .gap(4.0)
        .show(ui, |ui| {
            Text::new(label)
                .id_salt((label, "cell-caption"))
                .style(caption_style())
                .show(ui);
            Panel::zstack()
                .id_salt((label, "cell-body"))
                .size((Sizing::FILL, Sizing::FILL))
                .padding(8.0)
                .background(bg)
                .show(ui, body);
        });
}
