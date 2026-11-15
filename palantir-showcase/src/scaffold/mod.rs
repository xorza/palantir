//! The showcase's look outside palantir's widgets: tokens read from the live palette, the type
//! ramp, chips and the rail, the [`Kit`] a page paints with and its specimen
//! swatches, and the page scaffolding.
//!
//! [`Kit`]: kit::Kit
//!
//! Every scaffold color is a role of the palette the shown scheme wears, so the tour shows the
//! stock look in either scheme: the nav rail rides the surround band, pages sit on the window's
//! ground, demos in edged wells of the same ground (`LIGHT_WELL` when bright).
//!
//! Pages never paint their own root or header; they emit `Section`s, with fixed `TILE`-square
//! `DemoCell`s for paint demos.

pub(crate) mod chip;
pub(crate) mod demo_cell;
pub(crate) mod dialog;
pub(crate) mod kit;
pub(crate) mod knob;
pub(crate) mod per_scheme;
pub(crate) mod rail;
pub(crate) mod section;
pub(crate) mod tokens;

/// The checked spelling of an API a page shows: expands to the path as text after the compiler
/// resolves it, so a rename breaks the build.
///
/// - `api!(Ui::keep_open)`: a value path (function, method, associated const or unit variant).
/// - `api!(Ui::animate as fn(&mut Ui, ..) -> Animated<f32>)`: a generic function; the pointer type fixes
///   arguments a turbofish can't.
/// - `api!(type TabbedView)`: a type, trait or module item, for a method whose generic receiver
///   neither form can name.
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

use std::panic::Location;

use palantir::{
    Axis, Block, Border, Checkbox, Chrome, ColorScheme, Configure, Corners, Expander, FontWeight,
    Panel, RadioGroup, RadioOptions, RgbaF32, SizeSpec, Text, TextInput, TextStyle, TextWrap, Ui,
    VAlign, WidgetId, fmt,
};

use crate::scaffold::tokens::M;
use crate::scaffold::tokens::MEASURE;

use crate::scaffold::kit::Kit;

/// Bright demo surface, for drop shadows and dark strokes, in either scheme.
const LIGHT_WELL: RgbaF32 = RgbaF32::hex(0xc8ccd2);

/// The ink on a swatch, in either scheme: [`Swatches::solve`](kit::Swatches::solve) holds every
/// swatch to it.
const SWATCH_INK: RgbaF32 = RgbaF32::hex(0x14161a);

pub(crate) const PAGE_GAP: f32 = 2.0 * M.space.lg;
const TITLE_GAP: f32 = M.space.md;
pub(crate) const ROW_GAP: f32 = M.space.sm;
pub(crate) const TILE_GAP: f32 = M.space.md;
/// Edge of a square paint demo, a [`DemoCell`](demo_cell::DemoCell)'s default size.
pub(crate) const TILE: f32 = 168.0;
/// Height reserved for a tile caption; fixed so a two-line label doesn't misalign its tile.
const CAPTION_H: f32 = 30.0;
pub(crate) const CELL_PADDING: f32 = M.space.sm;
pub(crate) const RADIUS: f32 = M.radius.card;

/// A section's title: `Metrics`' 16 px title, semibold, as Fluent 2's Subtitle 2.
const fn section_style(k: Kit) -> TextStyle {
    k.t.body()
        .with_font_size(M.text.title)
        .with_weight(FontWeight::SEMI_BOLD)
}

/// [`SWATCH_INK`] at the caption size, for a label on a swatch fill.
pub(crate) const fn on_swatch_style() -> TextStyle {
    TextStyle::stock(ColorScheme::Dark)
        .with_font_size(M.text.caption)
        .with_color(SWATCH_INK)
}

pub(crate) fn swatch_chrome(c: RgbaF32) -> Chrome {
    Chrome::rounded(c, Corners::all(M.radius.control))
}

fn light_well_chrome() -> Chrome {
    Chrome::rounded(LIGHT_WELL, Corners::all(RADIUS))
}

/// Raised chrome: popup bodies, right-click targets, menu surfaces.
pub(crate) fn raised_chrome(k: Kit) -> Chrome {
    Chrome::rounded(k.t.raised, Corners::all(RADIUS))
        .with_border(Border::new(k.t.border, M.stroke.hairline))
}

/// Wrapping prose for demos whose rules can't be read off them, capped at a readable measure.
#[track_caller]
pub(crate) fn note(ui: &mut Ui, k: Kit, text: &'static str) {
    Text::new(text)
        .auto_id()
        .style(&k.t.caption())
        .size(SizeSpec::FILL_W)
        .max_size((MEASURE, f32::INFINITY))
        .text_wrap(TextWrap::WrapWithOverflow)
        .show(ui);
}

#[track_caller]
pub(crate) fn row(ui: &mut Ui, body: impl FnOnce(&mut Ui)) {
    Panel::hstack()
        .auto_id()
        .size(SizeSpec::FILL_W)
        .gap(ROW_GAP)
        .child_align(VAlign::Center)
        .show(ui, body);
}

/// A [`RadioGroup`] laid out as a [`row`]: its options left to right, spaced alike.
#[track_caller]
pub(crate) fn radio_row<T: PartialEq>(
    ui: &mut Ui,
    cur: &mut T,
    body: impl FnOnce(&mut Ui, &mut RadioOptions<'_, T>),
) {
    RadioGroup::new(cur)
        .auto_id()
        .axis(Axis::X)
        .gap(ROW_GAP)
        .child_align(VAlign::Center)
        .show(ui, body);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Col {
    Left,
    Right,
}

/// The two columns of a side-by-side page; one body runs for both, told which it fills.
#[track_caller]
pub(crate) fn cols(ui: &mut Ui, mut body: impl FnMut(&mut Ui, Col)) {
    Panel::hstack()
        .auto_id()
        .size(SizeSpec::FILL_W)
        .gap(2.0 * M.space.lg)
        .show(ui, |ui| {
            for col in [Col::Left, Col::Right] {
                Panel::vstack()
                    .id_salt(col as u8)
                    .gap(PAGE_GAP)
                    .size(SizeSpec::FILL_W)
                    .show(ui, |ui| body(ui, col));
            }
        });
}

#[track_caller]
pub(crate) fn well(ui: &mut Ui, k: Kit, body: impl FnOnce(&mut Ui)) {
    Panel::vstack()
        .auto_id()
        .size(SizeSpec::FILL_W)
        .padding(M.space.lg)
        .gap(M.space.md)
        .chrome(k.t.card())
        .show(ui, body);
}

/// One line of live state: a dim label and a monospace value.
#[track_caller]
pub(crate) fn readout<'a>(
    ui: &mut Ui,
    k: Kit,
    label: &'static str,
    value: impl Into<TextInput<'a>>,
) {
    Panel::hstack()
        .auto_id()
        .size(SizeSpec::FILL_W)
        .gap(M.space.sm)
        .show(ui, |ui| {
            Text::new(label)
                .style(&k.t.caption())
                .min_size((READOUT_LABEL_W, 0.0))
                .show(ui);
            Text::new(value).style(&k.t.mono(k.t.ink)).show(ui);
        });
}

const READOUT_LABEL_W: f32 = 96.0;

/// The checks a page leaves to a person, as they need what no harness drives: folded under one
/// "Check by hand (n)" disclosure, so they stay out of the demos' way. The ticks are keyed by call
/// site and kept for the `Ui`'s life, folded or not. At most 32 items (one bit each).
#[track_caller]
pub(crate) fn checklist(ui: &mut Ui, items: &[&'static str]) {
    debug_assert!(items.len() <= 32, "a checklist holds at most 32 items");
    let at = Location::caller();
    let title = fmt!(ui, "Check by hand ({})", items.len());
    Expander::new(title)
        .id(WidgetId::from_hash(("showcase::checklist", at)))
        .show(ui, |ui| {
            let ticks = WidgetId::from_hash(("showcase::checklist::ticks", at));
            ui.with_state::<Ticks, _>(ticks, |ui, ticks| {
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

/// A plain color swatch: a `Block` over [`swatch_chrome`], for the caller to key and size.
pub(crate) fn swatch(c: RgbaF32) -> Block {
    Block::new().chrome(swatch_chrome(c))
}

#[track_caller]
pub(crate) fn tiles(ui: &mut Ui, body: impl FnOnce(&mut Ui)) {
    Panel::wrap_hstack()
        .auto_id()
        .size(SizeSpec::FILL_W)
        .gap(TILE_GAP)
        .line_gap(TILE_GAP)
        .show(ui, body);
}

/// The least `x` in `0..=1` that `reaches` holds for, `reaches` turning true once as `x` rises
/// and holding at 1: 24 halvings, as many as an f32 mantissa holds bits.
pub(crate) fn bisect(reaches: impl Fn(f32) -> bool) -> f32 {
    let (mut lo, mut hi) = (0.0_f32, 1.0_f32);
    for _ in 0..24 {
        let mid = f32::midpoint(lo, hi);
        if reaches(mid) {
            hi = mid;
        } else {
            lo = mid;
        }
    }
    hi
}

#[cfg(test)]
mod tests;
