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
//! card, the title, the blurb and the key hints. A page emits
//! [`section`]s into the vstack it is handed — a short title, the APIs
//! it demonstrates, one [`note`] when the demo needs words — and paint
//! demos go in fixed [`TILE`]-square [`demo_cell`]s so every tile in the
//! app is the same size.

use palantir::internals::demo_swatches;
use std::hash::Hash;
use std::panic::Location;

use palantir::{
    Align, Background, Block, Checkbox, Configure, Corners, FontFamily, FontWeight, Panel, RgbaF32,
    Sizing, Stroke, Text, TextInput, TextStyle, TextWrap, Ui, VAlign, WidgetId,
};

/// Window clear — the darkest tier, visible around the card.
pub(crate) const WINDOW: RgbaF32 = RgbaF32::hex(0x131417);
/// Nav rail fill.
pub(crate) const SIDEBAR: RgbaF32 = RgbaF32::hex(0x1a1b1f);
/// Page canvas. Deliberately darker than [`ELEMENT`], the button fill this
/// showcase installs, so widgets read as raised against it.
pub(crate) const CARD: RgbaF32 = RgbaF32::hex(0x212329);
/// Recessed demo surface — one step below [`CARD`].
pub(crate) const WELL: RgbaF32 = RgbaF32::hex(0x16171b);
/// Bright demo surface, for content that only reads against light
/// (drop shadows, dark strokes).
pub(crate) const LIGHT_WELL: RgbaF32 = RgbaF32::hex(0xc8ccd2);
/// Raised chrome — popup bodies, right-click targets, menu surfaces.
pub(crate) const RAISED: RgbaF32 = RgbaF32::hex(0x2a2c33);
/// Border on raised surfaces and the card edge.
pub(crate) const BORDER: RgbaF32 = RgbaF32::hex(0x30333c);
/// Divider rules — one step quieter than [`BORDER`].
pub(crate) const HAIRLINE: RgbaF32 = RgbaF32::hex(0x272a31);

/// Widget-chrome fill at rest. `shell`'s palette reads this rung and the
/// three below it, so widget chrome and the page's own surfaces come off
/// one ladder — which only holds while the ladder lives in one file.
pub(crate) const ELEMENT: RgbaF32 = RgbaF32::hex(0x2b2e36);
/// Widget chrome under the pointer.
pub(crate) const ELEM_MID: RgbaF32 = RgbaF32::hex(0x353942);
/// Widget chrome while pressed.
pub(crate) const ELEM_STRONG: RgbaF32 = RgbaF32::hex(0x434854);
/// Focus ring on a widget border: a dimmer teal than [`ACCENT`], so the
/// ring reads as an edge rather than a fill.
pub(crate) const BORDER_FOCUSED: RgbaF32 = RgbaF32::hex(0x2b6f8f);

/// Primary ink.
pub(crate) const INK: RgbaF32 = RgbaF32::hex(0xe8eaf0);
/// Captions, readouts, secondary labels.
pub(crate) const INK_DIM: RgbaF32 = RgbaF32::hex(0x949bab);
/// Group headings and other structural chrome.
pub(crate) const INK_FAINT: RgbaF32 = RgbaF32::hex(0x6d7482);
/// Disabled labels — the quietest rung, below [`INK_FAINT`].
pub(crate) const INK_DISABLED: RgbaF32 = RgbaF32::hex(0x5f6673);
/// Selection / focus accent. *Is* swatch [`A`], rather than matching it
/// by a re-typed hex: highlights and demo content share one hue, and a
/// second literal is how two of them drift apart.
pub(crate) const ACCENT: RgbaF32 = A;

// Aliased from `palantir::internals::demo_swatches`, which the benchmark fixture
// reads too — the one set of ink both bundled demo surfaces use. Named
// A-E here because this page is a *tour*: what matters is that two
// chips differ, not what either means.
/// Teal-blue. Default swatch when one color is enough.
pub(crate) const A: RgbaF32 = demo_swatches::TEAL;
/// Orange. Pair with `A` for "two distinct things".
pub(crate) const B: RgbaF32 = demo_swatches::ORANGE;
/// Green.
pub(crate) const C: RgbaF32 = demo_swatches::LIME;
/// Purple.
pub(crate) const D: RgbaF32 = demo_swatches::VIOLET;
/// Red — the "wrong / danger" swatch.
pub(crate) const E: RgbaF32 = demo_swatches::RED;

/// Gap between a page's sections.
pub(crate) const PAGE_GAP: f32 = 28.0;
/// Gap between a section's header and its body.
const TITLE_GAP: f32 = 10.0;
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
/// The padding a demo cell puts between its surface and its body.
pub(crate) const CELL_PADDING: f32 = 8.0;
/// Corner radius for demo surfaces.
pub(crate) const RADIUS: f32 = 6.0;
/// The readable measure for prose: a note wraps here rather than running
/// the full width of the card.
const MEASURE: f32 = 680.0;

/// The checked spelling of an API a page demonstrates: expands to the
/// path as text, after making the compiler resolve it. A rename breaks the
/// build here instead of leaving a page naming something that is gone.
///
/// Three forms:
///
/// - `api!(Ui::keep_open)` — a value path: a function, a method (a trait
///   method through an implementor, `Panel::arrow_focus`), an associated
///   const, or a unit variant.
/// - `api!(Ui::animate as fn(&mut Ui, ..) -> f32)` — a generic function:
///   the pointer type fixes the arguments no turbofish can, `impl Trait`
///   ones included, and a trait method's `Self`
///   (`Configure::focusable as fn(Stepper<'static>, bool) -> ..`).
/// - `api!(type TabbedView)` — a type, a trait or a module item, for a
///   method whose generic receiver neither form above can name.
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

/// Page title, rendered by the shell.
pub(crate) fn title_style() -> TextStyle {
    TextStyle::default()
        .with_font_size(20.0)
        .with_weight(FontWeight::BOLD)
        .with_color(INK)
}

/// One-line page description under the title.
pub(crate) fn blurb_style() -> TextStyle {
    TextStyle::default()
        .with_font_size(13.0)
        .with_color(INK_DIM)
}

/// Section heading inside a page.
fn section_style() -> TextStyle {
    TextStyle::default()
        .with_font_size(14.0)
        .with_weight(FontWeight::BOLD)
        .with_color(INK)
}

/// Tile captions and other small secondary labels.
pub(crate) fn caption_style() -> TextStyle {
    TextStyle::default()
        .with_font_size(11.0)
        .with_color(INK_DIM)
}

/// Prose and live readouts.
pub(crate) fn note_style() -> TextStyle {
    TextStyle::default()
        .with_font_size(12.0)
        .with_color(INK_DIM)
}

/// Body copy inside a demo.
pub(crate) fn body_style() -> TextStyle {
    TextStyle::default().with_font_size(13.0).with_color(INK)
}

/// Monospace ink for values, key names and API paths.
pub(crate) fn mono_style(size: f32, color: RgbaF32) -> TextStyle {
    TextStyle::default()
        .with_family(FontFamily::MONO)
        .with_font_size(size)
        .with_color(color)
}

/// Near-black ink for placing on top of a bright swatch fill — a
/// legibility requirement, not decoration.
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

/// Standard swatch fill — colored rect with a 4 px corner radius.
pub(crate) fn swatch_bg(c: RgbaF32) -> Background {
    Background::rounded(c, Corners::all(4.0))
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
    Background::rounded(RAISED, Corners::all(8.0)).with_border(Stroke::new(BORDER, 1.0))
}

/// A titled block of demo content: a short title, then a chip per API it
/// demonstrates, then the body. The title names the subject in a few
/// words; anything the demo cannot say by being looked at goes in a
/// [`note`] at the top of the body. A note names an API only when one of
/// the section's [`api!`] chips checks it, so a rename cannot leave prose
/// naming something that is gone.
///
/// Identity comes from the call site, not from the title: the layout
/// helpers in this module are `#[track_caller]` and chain `.auto_id()`, so
/// each `section` in each page is a distinct widget without a page having
/// to invent a key for it — and editing a title doesn't re-key the
/// section's contents.
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

/// Wrapping prose for the rare demo whose rules can't be read off it by
/// looking. Capped at a readable measure.
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

/// Horizontal row of controls, hugging its content height, centred on
/// one line so a label sits on the axis of the control beside it.
#[track_caller]
pub(crate) fn row(ui: &mut Ui, body: impl FnOnce(&mut Ui)) {
    Panel::hstack()
        .auto_id()
        .size((Sizing::FILL, Sizing::HUG))
        .gap(ROW_GAP)
        .child_align(Align::v(VAlign::Center))
        .show(ui, body);
}

/// One side of a [`columns`] layout.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Column {
    Left,
    Right,
}

/// The two columns of a side-by-side page, each a `Fill`-wide `HUG`
/// stack at the page rhythm, top-aligned. One body runs for both, told
/// which column it fills, so both halves can borrow the same state.
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

/// The padded recessed stage a live demo sits on.
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

/// One line of "what the demo holds now": a dim label and a monospace
/// value, so live state reads the same on every page.
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

/// Wide enough for the longest readout label, so the values line up.
const READOUT_LABEL_W: f32 = 96.0;

/// Keycaps: the keys and gestures a demo answers to, set as chips rather
/// than buried in prose.
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

/// Ticks for a check made by hand: each item is what should happen, and
/// the box is where the tester says it did. The ticks live as long as the
/// `Ui`, keyed by the call site, so a page switch doesn't lose them.
///
/// At most 32 items: the ticks are one bit each.
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

/// One checklist's ticks, a bit per item.
#[derive(Default, Debug)]
struct Ticks(u32);

/// A plain colour swatch: a sized `Block` over [`swatch_bg`]. The leaf
/// most demo cells are built from, so the pages state only its size and
/// colour.
pub(crate) fn swatch<H: Hash>(ui: &mut Ui, id: H, size: (Sizing, Sizing), c: RgbaF32) {
    Block::new()
        .id_salt(id)
        .size(size)
        .background(swatch_bg(c))
        .show(ui);
}

/// Flowing line of demo tiles. Wraps rather than shrinking, so a tile is
/// the same size at every window width.
#[track_caller]
pub(crate) fn tiles(ui: &mut Ui, body: impl FnOnce(&mut Ui)) {
    Panel::wrap_hstack()
        .auto_id()
        .size((Sizing::FILL, Sizing::HUG))
        .gap(TILE_GAP)
        .line_gap(TILE_GAP)
        .show(ui, body);
}

/// Captioned [`TILE`]-square demo cell on the recessed surface.
#[track_caller]
pub(crate) fn demo_cell(ui: &mut Ui, label: &'static str, body: impl FnOnce(&mut Ui)) {
    demo_cell_on(ui, label, TILE, TILE, Some(well_bg()), body);
}

/// [`demo_cell`] on the bright surface — for shadow / dark-stroke content.
#[track_caller]
pub(crate) fn demo_cell_light(ui: &mut Ui, label: &'static str, body: impl FnOnce(&mut Ui)) {
    demo_cell_on(ui, label, TILE, TILE, Some(light_well_bg()), body);
}

/// [`demo_cell`] at a custom size, for the demos whose point only reads
/// in a box that isn't [`TILE`]-square.
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

/// Caption above a bare body — for demos that paint their own surface
/// (clip cards, gradients) where a recessed well would double up.
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

/// `#[track_caller]` all the way down: each of the four public cells forwards
/// here, so the outer panel's `.auto_id()` reads the *page's* call site rather
/// than any line in this file. Everything inside it — the caption, the body
/// cell — is parent-scoped under that panel and needs no id of its own.
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
