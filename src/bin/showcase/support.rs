//! Design tokens and page scaffolding — the single place the showcase's
//! look is defined, so every page reads as one app rather than twenty
//! separate demos.
//!
//! Three layers of surface, darkest to lightest: [`WINDOW`] behind
//! everything, [`SIDEBAR`] for the nav rail, [`CARD`] for the page
//! canvas. Demos recess into [`WELL`] (or [`LIGHT_WELL`] when the
//! content needs a bright backdrop) and raised chrome sits on
//! [`RAISED`] — always one step away from its parent so bounds read
//! without a border.
//!
//! Ink follows the same ladder: [`INK`] for content, [`INK_DIM`] for
//! captions and readouts, [`INK_FAINT`] for structural labels.
//!
//! Pages never paint their own root or header: the shell supplies the
//! card, the title, and the blurb. A page emits [`section`]s into the
//! vstack it is handed, and paint demos go in fixed [`TILE`]-square
//! [`demo_cell`]s so every tile in the app is the same size.

use palantir::{
    Background, Color, Configure, Corners, FontWeight, Panel, Sizing, Stroke, Text, TextStyle,
    TextWrap, Ui,
};
use std::hash::Hash;

/// Window clear — the darkest tier, visible around the card.
pub(crate) const WINDOW: Color = Color::hex(0x131417);
/// Nav rail fill.
pub(crate) const SIDEBAR: Color = Color::hex(0x1a1b1f);
/// Page canvas. Deliberately darker than the theme's `elem` button fill
/// (`#343434`) so widgets read as raised against it.
pub(crate) const CARD: Color = Color::hex(0x212329);
/// Recessed demo surface — one step below [`CARD`].
pub(crate) const WELL: Color = Color::hex(0x16171b);
/// Bright demo surface, for content that only reads against light
/// (drop shadows, dark strokes).
pub(crate) const LIGHT_WELL: Color = Color::hex(0xc8ccd2);
/// Raised chrome — popup bodies, right-click targets, menu surfaces.
pub(crate) const RAISED: Color = Color::hex(0x2a2c33);
/// Border on raised surfaces and the card edge.
pub(crate) const BORDER: Color = Color::hex(0x30333c);
/// Divider rules — one step quieter than [`BORDER`].
pub(crate) const HAIRLINE: Color = Color::hex(0x272a31);

/// Primary ink.
pub(crate) const INK: Color = Color::hex(0xe8eaf0);
/// Captions, readouts, secondary labels.
pub(crate) const INK_DIM: Color = Color::hex(0x949bab);
/// Group headings and other structural chrome.
pub(crate) const INK_FAINT: Color = Color::hex(0x6d7482);
/// Selection / focus accent. Matches swatch [`A`] so highlights and
/// demo content share one hue.
pub(crate) const ACCENT: Color = Color::hex(0x4cd3ff);

/// Teal-blue. Default swatch when one color is enough.
pub(crate) const A: Color = Color::hex(0x4cd3ff);
/// Orange. Pair with `A` for "two distinct things".
pub(crate) const B: Color = Color::hex(0xffa63d);
/// Green.
pub(crate) const C: Color = Color::hex(0xd9ff57);
/// Purple.
pub(crate) const D: Color = Color::hex(0xd897ff);
/// Red — the "wrong / danger" swatch.
pub(crate) const E: Color = Color::hex(0xff5e44);

/// Gap between a page's sections.
pub(crate) const PAGE_GAP: f32 = 20.0;
/// Gap between a section title and its body.
pub(crate) const TITLE_GAP: f32 = 8.0;
/// Gap between controls in a [`row`].
pub(crate) const ROW_GAP: f32 = 8.0;
/// Gap between demo tiles, along and across lines.
pub(crate) const TILE_GAP: f32 = 12.0;
/// Edge of a square paint demo. Every [`demo_cell`] in the app is this
/// size, so tiles line up across pages instead of dividing whatever
/// height each page happened to leave over.
pub(crate) const TILE: f32 = 168.0;
/// Height reserved for a tile caption. Fixed rather than hugging so a
/// two-line label doesn't push its tile's surface out of line with the
/// one-line labels beside it.
const CAPTION_H: f32 = 30.0;
/// Corner radius for demo surfaces.
pub(crate) const RADIUS: f32 = 6.0;

/// Page title, rendered by the shell.
pub(crate) fn title_style() -> TextStyle {
    TextStyle::default()
        .with_font_size(19.0)
        .with_weight(FontWeight::Bold)
        .with_color(INK)
}

/// One-line page description under the title.
pub(crate) fn blurb_style() -> TextStyle {
    TextStyle::default()
        .with_font_size(13.0)
        .with_color(INK_DIM)
}

/// Section heading inside a page.
pub(crate) fn section_style() -> TextStyle {
    TextStyle::default()
        .with_font_size(12.0)
        .with_weight(FontWeight::Bold)
        .with_color(INK)
}

/// Tile captions and other small secondary labels.
pub(crate) fn caption_style() -> TextStyle {
    TextStyle::default()
        .with_font_size(11.0)
        .with_color(INK_DIM)
}

/// Live readouts — "volume 60%", "last click: r3 c7".
pub(crate) fn note_style() -> TextStyle {
    TextStyle::default()
        .with_font_size(12.0)
        .with_color(INK_DIM)
}

/// Body copy inside a demo.
pub(crate) fn body_style() -> TextStyle {
    TextStyle::default().with_font_size(13.0).with_color(INK)
}

/// Near-black ink for placing on top of a bright swatch fill — a
/// legibility requirement, not decoration.
pub(crate) fn on_swatch_style() -> TextStyle {
    TextStyle::default()
        .with_font_size(12.0)
        .with_color(Color::hex(0x14161a))
}

/// Standard swatch fill — colored rect with a 4 px corner radius.
pub(crate) fn swatch_bg(c: Color) -> Background {
    Background {
        fill: c.into(),
        corners: Corners::all(4.0),
        ..Default::default()
    }
}

/// Recessed demo surface.
pub(crate) fn well_bg() -> Background {
    Background::rounded(WELL, Corners::all(RADIUS))
}

/// Bright demo surface — see [`LIGHT_WELL`].
pub(crate) fn light_well_bg() -> Background {
    Background::rounded(LIGHT_WELL, Corners::all(RADIUS))
}

/// Raised interactive surface: lifted fill + hairline edge.
pub(crate) fn raised_bg() -> Background {
    Background::rounded(RAISED, Corners::all(8.0)).with_stroke(Stroke::solid(BORDER, 1.0))
}

/// Section title above a block of demo content. The title is what makes
/// a page scannable, so every section has one.
pub(crate) fn section<H: Hash + Copy>(
    ui: &mut Ui,
    id: H,
    title: &'static str,
    body: impl FnOnce(&mut Ui),
) {
    Panel::vstack()
        .id_salt(id)
        .size((Sizing::FILL, Sizing::HUG))
        .gap(TITLE_GAP)
        .show(ui, |ui| {
            Text::new(title)
                .id_salt((id, "section-title"))
                .style(&section_style())
                .show(ui);
            body(ui);
        });
}

/// Wrapping prose under a section title, for the rare demo whose rules
/// can't be inferred from looking at it. Capped at a readable measure
/// rather than running the full width of the card.
pub(crate) fn note(ui: &mut Ui, text: &'static str) {
    Text::new(text)
        .id_salt(("note", text))
        .style(&note_style())
        .size((Sizing::FILL, Sizing::HUG))
        .max_size((680.0, f32::INFINITY))
        .text_wrap(TextWrap::WrapWithOverflow)
        .show(ui);
}

/// Horizontal row of controls, hugging its content height.
pub(crate) fn row<H: Hash + Copy>(ui: &mut Ui, id: H, body: impl FnOnce(&mut Ui)) {
    Panel::hstack()
        .id_salt(id)
        .size((Sizing::FILL, Sizing::HUG))
        .gap(ROW_GAP)
        .show(ui, body);
}

/// Flowing line of demo tiles. Wraps rather than shrinking, so a tile is
/// the same size at every window width.
pub(crate) fn tiles<H: Hash + Copy>(ui: &mut Ui, id: H, body: impl FnOnce(&mut Ui)) {
    Panel::wrap_hstack()
        .id_salt(id)
        .size((Sizing::FILL, Sizing::HUG))
        .gap(TILE_GAP)
        .line_gap(TILE_GAP)
        .show(ui, body);
}

/// Captioned [`TILE`]-square demo cell on the recessed surface.
pub(crate) fn demo_cell(ui: &mut Ui, label: &'static str, body: impl FnOnce(&mut Ui)) {
    demo_cell_on(ui, label, TILE, TILE, Some(well_bg()), body);
}

/// [`demo_cell`] on the bright surface — for shadow / dark-stroke content.
pub(crate) fn demo_cell_light(ui: &mut Ui, label: &'static str, body: impl FnOnce(&mut Ui)) {
    demo_cell_on(ui, label, TILE, TILE, Some(light_well_bg()), body);
}

/// [`demo_cell`] at a custom size, for the demos whose point only reads
/// in a box that isn't [`TILE`]-square.
pub(crate) fn demo_cell_at(
    ui: &mut Ui,
    label: &'static str,
    w: f32,
    h: f32,
    body: impl FnOnce(&mut Ui),
) {
    demo_cell_on(ui, label, w, h, Some(well_bg()), body);
}

/// Caption above a bare body — for demos that paint their own surface
/// (clip cards, gradients) where a recessed well would double up.
pub(crate) fn captioned_cell(
    ui: &mut Ui,
    label: &'static str,
    w: f32,
    h: f32,
    body: impl FnOnce(&mut Ui),
) {
    demo_cell_on(ui, label, w, h, None, body);
}

fn demo_cell_on(
    ui: &mut Ui,
    label: &'static str,
    w: f32,
    h: f32,
    bg: Option<Background>,
    body: impl FnOnce(&mut Ui),
) {
    Panel::vstack()
        .id_salt(label)
        .size((Sizing::fixed(w), Sizing::HUG))
        .gap(6.0)
        .show(ui, |ui| {
            caption(ui, label);
            let mut cell = Panel::zstack()
                .id_salt((label, "cell-body"))
                .size((Sizing::FILL, Sizing::fixed(h)));
            if let Some(bg) = bg {
                cell = cell.padding(8.0).background(bg);
            }
            cell.show(ui, body);
        });
}

fn caption(ui: &mut Ui, label: &'static str) {
    Text::new(label)
        .id_salt((label, "cell-caption"))
        .style(&caption_style())
        .size((Sizing::FILL, Sizing::fixed(CAPTION_H)))
        .text_wrap(TextWrap::WrapWithOverflow)
        .show(ui);
}
