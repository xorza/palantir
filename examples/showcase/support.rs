//! Design tokens and page scaffolding: the one place the showcase's look is defined.
//!
//! Surfaces, darkest to lightest: [`WINDOW`], [`SIDEBAR`], [`CARD`]; demos recess into [`WELL`] ([`LIGHT_WELL`] when bright), raised chrome sits on [`RAISED`]. Ink: [`INK`], [`INK_DIM`], [`INK_FAINT`].
//!
//! Pages never paint their own root or header; they emit [`section`]s, with fixed [`TILE`]-square [`demo_cell`]s for paint demos.

use palantir::internals::demo_swatches;
use std::hash::Hash;
use std::panic::Location;

use palantir::{
    Align, Background, Block, Checkbox, Configure, Corners, FontFamily, FontWeight, Panel, RgbaF32,
    Sizing, Stroke, Text, TextInput, TextStyle, TextWrap, Ui, VAlign, WidgetId,
};

/// Window clear; the darkest tier.
pub(crate) const WINDOW: RgbaF32 = RgbaF32::hex(0x131417);
/// Nav rail fill.
pub(crate) const SIDEBAR: RgbaF32 = RgbaF32::hex(0x1a1b1f);
/// Page canvas, darker than [`ELEMENT`] so widgets read as raised.
pub(crate) const CARD: RgbaF32 = RgbaF32::hex(0x212329);
/// Recessed demo surface — one step below [`CARD`].
pub(crate) const WELL: RgbaF32 = RgbaF32::hex(0x16171b);
/// Bright demo surface, for drop shadows and dark strokes.
pub(crate) const LIGHT_WELL: RgbaF32 = RgbaF32::hex(0xc8ccd2);
/// Raised chrome — popup bodies, right-click targets, menu surfaces.
pub(crate) const RAISED: RgbaF32 = RgbaF32::hex(0x2a2c33);
/// Border on raised surfaces and the card edge.
pub(crate) const BORDER: RgbaF32 = RgbaF32::hex(0x30333c);
/// Divider rules — one step quieter than [`BORDER`].
pub(crate) const HAIRLINE: RgbaF32 = RgbaF32::hex(0x272a31);

/// Widget-chrome fill at rest; `shell`'s palette reads this rung and the three below, so chrome and page share one ladder.
pub(crate) const ELEMENT: RgbaF32 = RgbaF32::hex(0x2b2e36);
/// Widget chrome under the pointer.
pub(crate) const ELEM_MID: RgbaF32 = RgbaF32::hex(0x353942);
/// Widget chrome while pressed.
pub(crate) const ELEM_STRONG: RgbaF32 = RgbaF32::hex(0x434854);
/// Focus ring on a widget border: dimmer than [`ACCENT`], so it reads as an edge.
pub(crate) const BORDER_FOCUSED: RgbaF32 = RgbaF32::hex(0x2b6f8f);

/// Primary ink.
pub(crate) const INK: RgbaF32 = RgbaF32::hex(0xe8eaf0);
/// Captions, readouts, secondary labels.
pub(crate) const INK_DIM: RgbaF32 = RgbaF32::hex(0x949bab);
pub(crate) const INK_FAINT: RgbaF32 = RgbaF32::hex(0x6d7482);
pub(crate) const INK_DISABLED: RgbaF32 = RgbaF32::hex(0x5f6673);
/// Selection / focus accent. *Is* swatch [`A`], not a re-typed hex, so the two cannot drift.
pub(crate) const ACCENT: RgbaF32 = A;

// Aliased from `palantir::internals::demo_swatches`, shared with the benchmark fixture.
pub(crate) const A: RgbaF32 = demo_swatches::TEAL;
pub(crate) const B: RgbaF32 = demo_swatches::ORANGE;
pub(crate) const C: RgbaF32 = demo_swatches::LIME;
pub(crate) const D: RgbaF32 = demo_swatches::VIOLET;
pub(crate) const E: RgbaF32 = demo_swatches::RED;

pub(crate) const PAGE_GAP: f32 = 28.0;
const TITLE_GAP: f32 = 10.0;
pub(crate) const ROW_GAP: f32 = 8.0;
pub(crate) const TILE_GAP: f32 = 12.0;
/// Edge of a square paint demo; every [`demo_cell`] is this size so tiles line up across pages.
pub(crate) const TILE: f32 = 168.0;
/// Height reserved for a tile caption; fixed so a two-line label doesn't misalign its tile.
const CAPTION_H: f32 = 30.0;
pub(crate) const CELL_PADDING: f32 = 8.0;
pub(crate) const RADIUS: f32 = 6.0;
const MEASURE: f32 = 680.0;

/// The checked spelling of an API a page demonstrates: expands to the path as text after the compiler resolves it, so a rename breaks the build.
///
/// Three forms:
///
/// - `api!(Ui::keep_open)`: a value path (function, method, associated const or unit variant).
/// - `api!(Ui::animate as fn(&mut Ui, ..) -> f32)`: a generic function; the pointer type fixes arguments a turbofish can't.
/// - `api!(type TabbedView)`: a type, trait or module item, for a method whose generic receiver neither form can name.
macro_rules! api {
    ($head:ident $(:: $tail:ident)* as $sig:ty) => {{
        let _: $sig = $head $(:: $tail)*;
        concat!(stringify!($head) $(, "::", stringify!($tail))*)
    }};
    (type $head:ident $(:: $tail:ident)*) => {{
        #[expect(unused_imports, reason = "the import is the check")]
        use $head $(:: $tail)* as _;
        concat!(stringify!($head) $(, "::", stringify!($tail))*)
    }};
    ($head:ident $(:: $tail:ident)*) => {{
        let _ = $head $(:: $tail)*;
        concat!(stringify!($head) $(, "::", stringify!($tail))*)
    }};
}
pub(crate) use api;

pub(crate) fn title_style() -> TextStyle {
    TextStyle::default()
        .with_font_size(20.0)
        .with_weight(FontWeight::BOLD)
        .with_color(INK)
}

pub(crate) fn blurb_style() -> TextStyle {
    TextStyle::default()
        .with_font_size(13.0)
        .with_color(INK_DIM)
}

fn section_style() -> TextStyle {
    TextStyle::default()
        .with_font_size(14.0)
        .with_weight(FontWeight::BOLD)
        .with_color(INK)
}

pub(crate) fn caption_style() -> TextStyle {
    TextStyle::default()
        .with_font_size(11.0)
        .with_color(INK_DIM)
}

pub(crate) fn note_style() -> TextStyle {
    TextStyle::default()
        .with_font_size(12.0)
        .with_color(INK_DIM)
}

pub(crate) fn body_style() -> TextStyle {
    TextStyle::default().with_font_size(13.0).with_color(INK)
}

pub(crate) fn mono_style(size: f32, color: RgbaF32) -> TextStyle {
    TextStyle::default()
        .with_family(FontFamily::MONO)
        .with_font_size(size)
        .with_color(color)
}

/// Near-black ink on a bright swatch fill; a legibility requirement.
pub(crate) fn on_swatch_style() -> TextStyle {
    TextStyle::default()
        .with_font_size(12.0)
        .with_color(RgbaF32::hex(0x14161a))
}

/// `a` moved toward `b` by `t`, per channel in linear RGB, alpha included.
pub(crate) const fn mix(a: RgbaF32, b: RgbaF32, t: f32) -> RgbaF32 {
    RgbaF32::new(
        a.r + (b.r - a.r) * t,
        a.g + (b.g - a.g) * t,
        a.b + (b.b - a.b) * t,
        a.a + (b.a - a.a) * t,
    )
}

pub(crate) fn swatch_bg(c: RgbaF32) -> Background {
    Background::rounded(c, Corners::all(4.0))
}

pub(crate) fn well_bg() -> Background {
    Background::rounded(WELL, Corners::all(RADIUS))
}

pub(crate) fn light_well_bg() -> Background {
    Background::rounded(LIGHT_WELL, Corners::all(RADIUS))
}

pub(crate) fn raised_bg() -> Background {
    Background::rounded(RAISED, Corners::all(8.0)).with_border(Stroke::new(BORDER, 1.0))
}

/// A titled block of demo content: a title, a chip per demonstrated API, then the body. A [`note`] may name an API only when one of the section's [`api!`] chips checks it.
///
/// Identity comes from the call site (`#[track_caller]` plus `.auto_id()`), so editing a title doesn't re-key the contents.
#[track_caller]
pub(crate) fn section(
    ui: &mut Ui,
    title: &'static str,
    apis: &[&'static str],
    body: impl FnOnce(&mut Ui),
) {
    Panel::vstack()
        .auto_id()
        .size((Sizing::FILL, Sizing::HUG))
        .gap(TITLE_GAP)
        .show(ui, |ui| {
            Panel::wrap_hstack()
                .size((Sizing::FILL, Sizing::HUG))
                .gap(6.0)
                .line_gap(6.0)
                .child_align(Align::v(VAlign::Center))
                .show(ui, |ui| {
                    Text::new(title)
                        .style(&section_style())
                        .margin((0.0, 0.0, 6.0, 0.0))
                        .show(ui);
                    for &api in apis {
                        chip(
                            ui,
                            api,
                            INK_DIM,
                            Background::rounded(RAISED, Corners::all(4.0)),
                        );
                    }
                });
            body(ui);
        });
}

/// Wrapping prose for demos whose rules can't be read off them, capped at a readable measure.
#[track_caller]
pub(crate) fn note(ui: &mut Ui, text: &'static str) {
    Text::new(text)
        .auto_id()
        .style(&note_style())
        .size((Sizing::FILL, Sizing::HUG))
        .max_size((MEASURE, f32::INFINITY))
        .text_wrap(TextWrap::WrapWithOverflow)
        .show(ui);
}

#[track_caller]
pub(crate) fn row(ui: &mut Ui, body: impl FnOnce(&mut Ui)) {
    Panel::hstack()
        .auto_id()
        .size((Sizing::FILL, Sizing::HUG))
        .gap(ROW_GAP)
        .child_align(Align::v(VAlign::Center))
        .show(ui, body);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Column {
    Left,
    Right,
}

/// The two columns of a side-by-side page; one body runs for both, told which it fills.
#[track_caller]
pub(crate) fn columns(ui: &mut Ui, mut body: impl FnMut(&mut Ui, Column)) {
    Panel::hstack()
        .auto_id()
        .size((Sizing::FILL, Sizing::HUG))
        .gap(32.0)
        .show(ui, |ui| {
            for column in [Column::Left, Column::Right] {
                Panel::vstack()
                    .id_salt(column as u8)
                    .gap(PAGE_GAP)
                    .size((Sizing::FILL, Sizing::HUG))
                    .show(ui, |ui| body(ui, column));
            }
        });
}

#[track_caller]
pub(crate) fn well(ui: &mut Ui, body: impl FnOnce(&mut Ui)) {
    Panel::vstack()
        .auto_id()
        .size((Sizing::FILL, Sizing::HUG))
        .padding(14.0)
        .gap(10.0)
        .background(well_bg())
        .show(ui, body);
}

/// One line of live state: a dim label and a monospace value.
#[track_caller]
pub(crate) fn readout<'a>(ui: &mut Ui, label: &'static str, value: impl Into<TextInput<'a>>) {
    Panel::hstack()
        .auto_id()
        .size((Sizing::FILL, Sizing::HUG))
        .gap(8.0)
        .show(ui, |ui| {
            Text::new(label)
                .style(&note_style())
                .min_size((READOUT_LABEL_W, 0.0))
                .show(ui);
            Text::new(value).style(&mono_style(12.0, INK)).show(ui);
        });
}

const READOUT_LABEL_W: f32 = 96.0;

/// Keycaps: the keys and gestures a demo answers to, as chips.
#[track_caller]
pub(crate) fn keys(ui: &mut Ui, keys: &[&'static str]) {
    Panel::wrap_hstack()
        .auto_id()
        .size((Sizing::FILL, Sizing::HUG))
        .gap(4.0)
        .line_gap(4.0)
        .show(ui, |ui| {
            for &key in keys {
                chip(
                    ui,
                    key,
                    INK,
                    Background::rounded(ELEMENT, Corners::all(4.0))
                        .with_border(Stroke::new(ELEM_STRONG, 1.0)),
                );
            }
        });
}

fn chip(ui: &mut Ui, text: &'static str, ink: RgbaF32, bg: Background) {
    Panel::hstack()
        .id_salt(text)
        .padding((6.0, 2.0))
        .background(bg)
        .show(ui, |ui| {
            Text::new(text).style(&mono_style(11.0, ink)).show(ui);
        });
}

/// Ticks for a hand check, keyed by call site and kept for the `Ui`'s life. At most 32 items (one bit each).
#[track_caller]
pub(crate) fn checklist(ui: &mut Ui, items: &[&'static str]) {
    debug_assert!(items.len() <= 32, "a checklist holds at most 32 items");
    let id = WidgetId::from_hash(("showcase::checklist", Location::caller()));
    Panel::vstack()
        .auto_id()
        .size((Sizing::FILL, Sizing::HUG))
        .padding(12.0)
        .gap(6.0)
        .background(
            Background::rounded(RgbaF32::TRANSPARENT, Corners::all(RADIUS))
                .with_border(Stroke::new(BORDER, 1.0)),
        )
        .show(ui, |ui| {
            Text::new("CHECK BY HAND")
                .style(
                    &TextStyle::default()
                        .with_font_size(10.0)
                        .with_weight(FontWeight::BOLD)
                        .with_color(INK_FAINT),
                )
                .show(ui);
            ui.with_state::<Ticks, _>(id, |ui, ticks| {
                for (i, &item) in items.iter().enumerate() {
                    let bit = 1 << i;
                    let mut done = ticks.0 & bit != 0;
                    Checkbox::new(&mut done).id_salt(i).label(item).show(ui);
                    ticks.0 = if done { ticks.0 | bit } else { ticks.0 & !bit };
                }
            });
        });
}

#[derive(Default, Debug)]
struct Ticks(u32);

/// A plain colour swatch: a sized `Block` over [`swatch_bg`].
pub(crate) fn swatch<H: Hash>(ui: &mut Ui, id: H, size: (Sizing, Sizing), c: RgbaF32) {
    Block::new()
        .id_salt(id)
        .size(size)
        .background(swatch_bg(c))
        .show(ui);
}

#[track_caller]
pub(crate) fn tiles(ui: &mut Ui, body: impl FnOnce(&mut Ui)) {
    Panel::wrap_hstack()
        .auto_id()
        .size((Sizing::FILL, Sizing::HUG))
        .gap(TILE_GAP)
        .line_gap(TILE_GAP)
        .show(ui, body);
}

#[track_caller]
pub(crate) fn demo_cell(ui: &mut Ui, label: &'static str, body: impl FnOnce(&mut Ui)) {
    demo_cell_on(ui, label, TILE, TILE, Some(well_bg()), body);
}

#[track_caller]
pub(crate) fn demo_cell_light(ui: &mut Ui, label: &'static str, body: impl FnOnce(&mut Ui)) {
    demo_cell_on(ui, label, TILE, TILE, Some(light_well_bg()), body);
}

/// [`demo_cell`] at a custom size, for demos that don't fit a [`TILE`] square.
#[track_caller]
pub(crate) fn demo_cell_at(
    ui: &mut Ui,
    label: &'static str,
    w: f32,
    h: f32,
    body: impl FnOnce(&mut Ui),
) {
    demo_cell_on(ui, label, w, h, Some(well_bg()), body);
}

#[track_caller]
pub(crate) fn captioned_cell(
    ui: &mut Ui,
    label: &'static str,
    w: f32,
    h: f32,
    body: impl FnOnce(&mut Ui),
) {
    demo_cell_on(ui, label, w, h, None, body);
}

/// `#[track_caller]` all the way down so the outer panel's `.auto_id()` reads the page's call site.
#[track_caller]
fn demo_cell_on(
    ui: &mut Ui,
    label: &'static str,
    w: f32,
    h: f32,
    bg: Option<Background>,
    body: impl FnOnce(&mut Ui),
) {
    Panel::vstack()
        .auto_id()
        .size((Sizing::fixed(w), Sizing::HUG))
        .gap(6.0)
        .show(ui, |ui| {
            Text::new(label)
                .style(&caption_style())
                .size((Sizing::FILL, Sizing::fixed(CAPTION_H)))
                .text_wrap(TextWrap::WrapWithOverflow)
                .show(ui);
            let mut cell = Panel::zstack().size((Sizing::FILL, Sizing::fixed(h)));
            if let Some(bg) = bg {
                cell = cell.padding(CELL_PADDING).background(bg);
            }
            cell.show(ui, body);
        });
}
