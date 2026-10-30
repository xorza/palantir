//! Design tokens and page scaffolding: the one place the showcase's look is defined.
//!
//! Every scaffold color is a role of [`Palette::DEFAULT`], the palette `Theme::default` assembles from, so the tour shows the stock look: the nav rail rides the [`SURROUND`] band, pages sit on the window's ground, demos in hairline-framed [`WELL`]s of the same ground ([`LIGHT_WELL`] when bright), raised chrome on [`RAISED`]. Ink: [`INK`], [`INK_DIM`], [`INK_FAINT`].
//!
//! Pages never paint their own root or header; they emit [`Section`]s, with fixed [`TILE`]-square [`DemoCell`]s for paint demos.

use palantir::internals::demo_swatches;
use std::panic::Location;

use palantir::{
    Align, Block, Checkbox, Chrome, Configure, Corners, FontFamily, FontWeight, Metrics, Palette,
    Panel, RgbaF32, Sizing, Stroke, Text, TextInput, TextStyle, TextWrap, Ui, VAlign, WidgetId,
};

/// The stock palette and steps every token below reads.
const P: Palette = Palette::DEFAULT;
const M: Metrics = Metrics::DEFAULT;

/// A demo's surface: the ground itself, so a tab strip's selected chip, which wears the ground,
/// joins the page under it as it does in an app. A hairline frames it.
pub(crate) const WELL: RgbaF32 = P.window_bg;
/// The band the nav rail rides, as a menu bar and a tab strip do.
pub(crate) const SURROUND: RgbaF32 = P.surround;
/// Bright demo surface, for drop shadows and dark strokes.
pub(crate) const LIGHT_WELL: RgbaF32 = RgbaF32::hex(0xc8ccd2);
/// Raised chrome — popup bodies, right-click targets, menu surfaces.
pub(crate) const RAISED: RgbaF32 = P.element;
/// Border on raised surfaces and around a well.
pub(crate) const BORDER: RgbaF32 = P.border_mid();
/// Divider rules — one step quieter than [`BORDER`].
pub(crate) const HAIRLINE: RgbaF32 = P.border_soft();

/// Widget-chrome fill at rest.
pub(crate) const ELEMENT: RgbaF32 = P.element;
/// Widget chrome under the pointer.
pub(crate) const ELEM_MID: RgbaF32 = P.element_mid;
/// Widget chrome while pressed.
pub(crate) const ELEM_STRONG: RgbaF32 = P.element_strong;

/// Primary ink.
pub(crate) const INK: RgbaF32 = P.text;
/// Captions, readouts, secondary labels.
pub(crate) const INK_DIM: RgbaF32 = P.text_muted;
/// Group headings and the quietest labels.
pub(crate) const INK_FAINT: RgbaF32 = P.text_disabled;
/// Selection / focus accent.
pub(crate) const ACCENT: RgbaF32 = P.accent;

// Aliased from `palantir::internals::demo_swatches`, shared with the benchmark fixture.
pub(crate) const A: RgbaF32 = demo_swatches::TEAL;
pub(crate) const B: RgbaF32 = demo_swatches::ORANGE;
pub(crate) const C: RgbaF32 = demo_swatches::LIME;
pub(crate) const D: RgbaF32 = demo_swatches::VIOLET;
pub(crate) const E: RgbaF32 = demo_swatches::RED;

pub(crate) const PAGE_GAP: f32 = 2.0 * M.space.lg;
const TITLE_GAP: f32 = M.space.md;
pub(crate) const ROW_GAP: f32 = M.space.sm;
pub(crate) const TILE_GAP: f32 = M.space.md;
/// Edge of a square paint demo, a [`DemoCell`]'s default size.
pub(crate) const TILE: f32 = 168.0;
/// Height reserved for a tile caption; fixed so a two-line label doesn't misalign its tile.
const CAPTION_H: f32 = 30.0;
pub(crate) const CELL_PADDING: f32 = M.space.sm;
pub(crate) const RADIUS: f32 = M.radius.card;
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

/// The type ramp the scaffold reads, around the stock 14 px body: a caption two under it, a
/// section's subtitle two over, a page's title.
const CAPTION_PX: f32 = 12.0;
const SUBTITLE_PX: f32 = 16.0;
const TITLE_PX: f32 = 20.0;

pub(crate) fn title_style() -> TextStyle {
    TextStyle::default()
        .with_font_size(TITLE_PX)
        .with_weight(FontWeight::BOLD)
}

pub(crate) fn blurb_style() -> TextStyle {
    TextStyle::default().with_color(INK_DIM)
}

fn section_style() -> TextStyle {
    TextStyle::default()
        .with_font_size(SUBTITLE_PX)
        .with_weight(FontWeight::BOLD)
}

pub(crate) fn caption_style() -> TextStyle {
    TextStyle::default()
        .with_font_size(CAPTION_PX)
        .with_color(INK_DIM)
}

pub(crate) fn note_style() -> TextStyle {
    caption_style()
}

/// The stock ambient style, as an unstyled `Text` wears it.
pub(crate) fn body_style() -> TextStyle {
    TextStyle::default()
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
        .with_font_size(CAPTION_PX)
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

pub(crate) fn swatch_chrome(c: RgbaF32) -> Chrome {
    Chrome::rounded(c, Corners::all(M.radius.control))
}

pub(crate) fn well_chrome() -> Chrome {
    Chrome::rounded(WELL, Corners::all(RADIUS))
        .with_border(Stroke::new(HAIRLINE, M.stroke.hairline))
}

pub(crate) fn light_well_chrome() -> Chrome {
    Chrome::rounded(LIGHT_WELL, Corners::all(RADIUS))
}

pub(crate) fn raised_chrome() -> Chrome {
    Chrome::rounded(RAISED, Corners::all(RADIUS))
        .with_border(Stroke::new(BORDER, M.stroke.hairline))
}

/// A titled block of demo content: a title, a chip per demonstrated API, then the body. A [`note`] may name an API only when one of the section's [`api!`] chips checks it.
#[derive(Debug)]
#[must_use]
pub(crate) struct Section<'a> {
    title: &'static str,
    apis: &'a [&'static str],
}

impl<'a> Section<'a> {
    pub(crate) const fn new(title: &'static str) -> Self {
        Self { title, apis: &[] }
    }

    /// The APIs the section demonstrates, each spelled by [`api!`].
    pub(crate) const fn apis(mut self, apis: &'a [&'static str]) -> Self {
        self.apis = apis;
        self
    }

    /// Identity comes from the call site (`#[track_caller]` plus `.auto_id()`), so editing a title doesn't re-key the contents.
    #[track_caller]
    pub(crate) fn show(self, ui: &mut Ui, body: impl FnOnce(&mut Ui)) {
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
                        Text::new(self.title)
                            .style(&section_style())
                            .margin((0.0, 0.0, 6.0, 0.0))
                            .show(ui);
                        for &api in self.apis {
                            chip(ui, api, Chip::Api);
                        }
                    });
                body(ui);
            });
    }
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
pub(crate) enum Col {
    Left,
    Right,
}

/// The two columns of a side-by-side page; one body runs for both, told which it fills.
#[track_caller]
pub(crate) fn cols(ui: &mut Ui, mut body: impl FnMut(&mut Ui, Col)) {
    Panel::hstack()
        .auto_id()
        .size((Sizing::FILL, Sizing::HUG))
        .gap(32.0)
        .show(ui, |ui| {
            for col in [Col::Left, Col::Right] {
                Panel::vstack()
                    .id_salt(col as u8)
                    .gap(PAGE_GAP)
                    .size((Sizing::FILL, Sizing::HUG))
                    .show(ui, |ui| body(ui, col));
            }
        });
}

#[track_caller]
pub(crate) fn well(ui: &mut Ui, body: impl FnOnce(&mut Ui)) {
    Panel::vstack()
        .auto_id()
        .size((Sizing::FILL, Sizing::HUG))
        .padding(M.space.lg)
        .gap(M.space.md)
        .chrome(well_chrome())
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
            Text::new(value)
                .style(&mono_style(CAPTION_PX, INK))
                .show(ui);
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
                chip(ui, key, Chip::Key);
            }
        });
}

/// What a [`chip`] names.
#[derive(Clone, Copy, Debug)]
enum Chip {
    /// An API a [`Section`] demonstrates.
    Api,
    /// A key or gesture a page answers to.
    Key,
}

impl Chip {
    const fn ink(self) -> RgbaF32 {
        match self {
            Self::Api => INK_DIM,
            Self::Key => INK,
        }
    }

    fn chrome(self) -> Chrome {
        let corners = Corners::all(M.radius.control);
        match self {
            Self::Api => Chrome::rounded(RAISED, corners),
            Self::Key => Chrome::rounded(ELEMENT, corners)
                .with_border(Stroke::new(ELEM_STRONG, M.stroke.hairline)),
        }
    }
}

fn chip(ui: &mut Ui, text: &'static str, kind: Chip) {
    Panel::hstack()
        .id_salt(text)
        .padding((6.0, 2.0))
        .chrome(kind.chrome())
        .show(ui, |ui| {
            Text::new(text)
                .style(&mono_style(CAPTION_PX, kind.ink()))
                .show(ui);
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
        .padding(M.space.md)
        .gap(M.space.sm)
        .chrome(
            Chrome::rounded(RgbaF32::TRANSPARENT, Corners::all(RADIUS))
                .with_border(Stroke::new(BORDER, M.stroke.hairline)),
        )
        .show(ui, |ui| {
            Text::new("CHECK BY HAND").style(&heading_style()).show(ui);
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

/// An all-caps group label: the caption size, bold, in the faint ink.
pub(crate) fn heading_style() -> TextStyle {
    TextStyle::default()
        .with_font_size(CAPTION_PX)
        .with_weight(FontWeight::BOLD)
        .with_color(INK_FAINT)
}

/// A plain color swatch: a `Block` over [`swatch_chrome`], for the caller to key and size.
pub(crate) fn swatch(c: RgbaF32) -> Block {
    Block::new().chrome(swatch_chrome(c))
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

/// What a [`DemoCell`]'s body sits on.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Surface {
    /// A padded [`WELL`].
    Well,
    /// A padded [`LIGHT_WELL`], for drop shadows and dark strokes.
    LightWell,
    /// Nothing: the body fills the cell edge to edge.
    Bare,
}

/// A captioned paint demo, [`TILE`] square on a [`Surface::Well`] unless set otherwise, so tiles line up across pages. Keyed by its label, so a page can draw cells in a loop; cells in one row need distinct labels.
#[derive(Debug)]
#[must_use]
pub(crate) struct DemoCell {
    label: &'static str,
    w: f32,
    h: f32,
    surface: Surface,
}

impl DemoCell {
    pub(crate) const fn new(label: &'static str) -> Self {
        Self {
            label,
            w: TILE,
            h: TILE,
            surface: Surface::Well,
        }
    }

    /// The cell's width and body height, for demos that don't fit a [`TILE`] square.
    pub(crate) const fn size(mut self, w: f32, h: f32) -> Self {
        self.w = w;
        self.h = h;
        self
    }

    pub(crate) const fn surface(mut self, surface: Surface) -> Self {
        self.surface = surface;
        self
    }

    pub(crate) fn show(self, ui: &mut Ui, body: impl FnOnce(&mut Ui)) {
        Panel::vstack()
            .id_salt(self.label)
            .size((Sizing::fixed(self.w), Sizing::HUG))
            .gap(6.0)
            .show(ui, |ui| {
                Text::new(self.label)
                    .style(&caption_style())
                    .size((Sizing::FILL, Sizing::fixed(CAPTION_H)))
                    .text_wrap(TextWrap::WrapWithOverflow)
                    .show(ui);
                let cell = Panel::zstack().size((Sizing::FILL, Sizing::fixed(self.h)));
                let cell = match self.surface {
                    Surface::Well => cell.padding(CELL_PADDING).chrome(well_chrome()),
                    Surface::LightWell => cell.padding(CELL_PADDING).chrome(light_well_chrome()),
                    Surface::Bare => cell,
                };
                cell.show(ui, body);
            });
    }
}
