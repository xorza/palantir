//! The showcase's look over the shared scaffold: the [`Kit`] a page paints with, its specimen
//! [`Swatches`], and the page scaffolding.
//!
//! Every scaffold color is a role of the palette the shown scheme wears, so the tour shows the
//! stock look in either scheme: the nav rail rides the surround band, pages sit on the window's
//! ground, demos in edged wells of the same ground ([`LIGHT_WELL`] when bright).
//!
//! Pages never paint their own root or header; they emit [`Section`]s, with fixed [`TILE`]-square
//! [`DemoCell`]s for paint demos.

use std::panic::Location;

use palantir::{
    Align, Block, Checkbox, Chrome, ColorScheme, Configure, Corners, FontWeight, Metrics, Okhsv,
    Palette, Panel, RgbaF32, Sizing, Stroke, Text, TextInput, TextStyle, TextWrap, Ui, VAlign,
    WidgetId,
};

use crate::scaffold::header::Chip;
use crate::scaffold::tokens::{MEASURE, Tokens};

const M: Metrics = Metrics::DEFAULT;

/// Bright demo surface, for drop shadows and dark strokes, in either scheme.
const LIGHT_WELL: RgbaF32 = RgbaF32::hex(0xc8ccd2);

/// The ink on a swatch, in either scheme: [`Swatches::solve`] holds every swatch to it.
const SWATCH_INK: RgbaF32 = RgbaF32::hex(0x14161a);

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

/// What a page paints with this frame: the scaffold's tokens and the shown scheme's swatches.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Kit {
    pub(crate) t: Tokens,
    pub(crate) swatch: Swatches,
}

/// Both schemes' swatches, solved once, and the kit of whichever scheme shows.
#[derive(Debug)]
pub(crate) struct Kits {
    light: Swatches,
    dark: Swatches,
}

impl Kits {
    pub(crate) fn new() -> Self {
        Self {
            light: Swatches::solve(ColorScheme::Light),
            dark: Swatches::solve(ColorScheme::Dark),
        }
    }

    /// The kit of `scheme`, over the stock palette the tour wears.
    pub(crate) const fn kit(&self, scheme: ColorScheme) -> Kit {
        Kit {
            t: Tokens::of(Palette::for_scheme(scheme)),
            swatch: match scheme {
                ColorScheme::Light => self.light,
                ColorScheme::Dark => self.dark,
            },
        }
    }
}

/// The specimen colors of one scheme: five hues at one saturation and one relative luminance.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Swatches {
    pub(crate) teal: RgbaF32,
    pub(crate) orange: RgbaF32,
    pub(crate) lime: RgbaF32,
    pub(crate) violet: RgbaF32,
    pub(crate) red: RgbaF32,
}

/// What a scheme's swatches are solved to.
#[derive(Clone, Copy, Debug)]
struct Target {
    /// The Okhsv saturation every hue takes.
    saturation: f32,
    /// The relative luminance every swatch lands on.
    luminance: f32,
}

impl Target {
    /// Dark: the highest luminance every hue reaches at the saturation, as violet tops out at
    /// 0.368 there. Light: between the floor [`SWATCH_INK`] sets and the ceiling the light ground
    /// sets. The tests hold each swatch to 3:1 against its ground and 4.5:1 under
    /// [`SWATCH_INK`] (WCAG 1.4.11 and 1.4.3).
    const fn of(scheme: ColorScheme) -> Self {
        match scheme {
            ColorScheme::Dark => Self {
                saturation: 0.6,
                luminance: 0.35,
            },
            ColorScheme::Light => Self {
                saturation: 0.75,
                luminance: 0.25,
            },
        }
    }

    fn color(self, hue: f32) -> RgbaF32 {
        let at = |value| Okhsv::new(hue, self.saturation, value).to_color();
        debug_assert!(
            at(1.0).luminance() >= self.luminance,
            "hue {hue} cannot reach the target luminance"
        );
        let (mut lo, mut hi) = (0.0_f32, 1.0_f32);
        for _ in 0..BISECTION_STEPS {
            let mid = f32::midpoint(lo, hi);
            if at(mid).luminance() < self.luminance {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        at(hi)
    }
}

/// Halvings of the value's `0..=1` range, as many as an f32 mantissa holds bits.
const BISECTION_STEPS: u32 = 24;

impl Swatches {
    /// The Okhsv hues of the tour's first, neon set, in turns: teal, orange, lime, violet, red.
    const HUES: [f32; 5] = [0.62, 0.18, 0.34, 0.87, 0.09];

    /// Each hue at the scheme's saturation, its value solved by bisection to the scheme's
    /// luminance: one luminance gives every swatch one contrast against the ground and under
    /// [`SWATCH_INK`], as WCAG measures contrast in luminance, and Okhsv keeps the hue and the
    /// gamut as the value moves.
    pub(crate) fn solve(scheme: ColorScheme) -> Self {
        let target = Target::of(scheme);
        let [teal, orange, lime, violet, red] = Self::HUES.map(|hue| target.color(hue));
        Self {
            teal,
            orange,
            lime,
            violet,
            red,
        }
    }

    pub(crate) const fn all(self) -> [RgbaF32; 5] {
        [self.teal, self.orange, self.lime, self.violet, self.red]
    }
}

/// A section's title: `Metrics`' 16 px title, semibold, as Fluent 2's Subtitle 2.
fn section_style(k: Kit) -> TextStyle {
    k.t.body()
        .with_font_size(M.text.title)
        .with_weight(FontWeight::SEMI_BOLD)
}

/// [`SWATCH_INK`] at the caption size, for a label on a swatch fill.
pub(crate) fn on_swatch_style() -> TextStyle {
    TextStyle::default()
        .with_font_size(M.text.caption)
        .with_color(SWATCH_INK)
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

fn light_well_chrome() -> Chrome {
    Chrome::rounded(LIGHT_WELL, Corners::all(RADIUS))
}

/// Raised chrome: popup bodies, right-click targets, menu surfaces.
pub(crate) fn raised_chrome(k: Kit) -> Chrome {
    Chrome::rounded(k.t.raised, Corners::all(RADIUS))
        .with_border(Stroke::new(k.t.border, M.stroke.hairline))
}

/// A titled block of demo content: a title, a chip per demonstrated API, then the body. A [`note`]
/// may name an API only when one of the section's [`api!`](crate::scaffold::api) chips checks it.
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

    /// The APIs the section demonstrates, each spelled by [`api!`](crate::scaffold::api).
    pub(crate) const fn apis(mut self, apis: &'a [&'static str]) -> Self {
        self.apis = apis;
        self
    }

    /// Identity comes from the call site (`#[track_caller]` plus `.auto_id()`), so editing a title
    /// doesn't re-key the contents.
    #[track_caller]
    pub(crate) fn show(self, ui: &mut Ui, k: Kit, body: impl FnOnce(&mut Ui)) {
        Panel::vstack()
            .auto_id()
            .size((Sizing::FILL, Sizing::HUG))
            .gap(TITLE_GAP)
            .show(ui, |ui| {
                Panel::wrap_hstack()
                    .size((Sizing::FILL, Sizing::HUG))
                    .gap(M.space.xs)
                    .line_gap(M.space.xs)
                    .child_align(Align::v(VAlign::Center))
                    .show(ui, |ui| {
                        Text::new(self.title)
                            .style(&section_style(k))
                            .margin((0.0, 0.0, M.space.xs, 0.0))
                            .show(ui);
                        for &api in self.apis {
                            Chip::Tag.show(ui, k.t, api);
                        }
                    });
                body(ui);
            });
    }
}

/// Wrapping prose for demos whose rules can't be read off them, capped at a readable measure.
#[track_caller]
pub(crate) fn note(ui: &mut Ui, k: Kit, text: &'static str) {
    Text::new(text)
        .auto_id()
        .style(&k.t.caption())
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
        .gap(2.0 * M.space.lg)
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
pub(crate) fn well(ui: &mut Ui, k: Kit, body: impl FnOnce(&mut Ui)) {
    Panel::vstack()
        .auto_id()
        .size((Sizing::FILL, Sizing::HUG))
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
        .size((Sizing::FILL, Sizing::HUG))
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

/// Ticks for a hand check, keyed by call site and kept for the `Ui`'s life. At most 32 items (one
/// bit each).
#[track_caller]
pub(crate) fn checklist(ui: &mut Ui, k: Kit, items: &[&'static str]) {
    debug_assert!(items.len() <= 32, "a checklist holds at most 32 items");
    let id = WidgetId::from_hash(("showcase::checklist", Location::caller()));
    Panel::vstack()
        .auto_id()
        .size((Sizing::FILL, Sizing::HUG))
        .padding(M.space.md)
        .gap(M.space.sm)
        .chrome(
            Chrome::rounded(RgbaF32::TRANSPARENT, Corners::all(RADIUS))
                .with_border(Stroke::new(k.t.border, M.stroke.hairline)),
        )
        .show(ui, |ui| {
            Text::new("CHECK BY HAND").style(&k.t.heading()).show(ui);
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
    /// A padded well of the ground.
    Well,
    /// A padded [`LIGHT_WELL`], for drop shadows and dark strokes.
    LightWell,
    /// Nothing: the body fills the cell edge to edge.
    Bare,
}

/// A captioned paint demo, [`TILE`] square on a [`Surface::Well`] unless set otherwise, so tiles
/// line up across pages. Keyed by its label, so a page can draw cells in a loop; cells in one row
/// need distinct labels.
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

    pub(crate) fn show(self, ui: &mut Ui, k: Kit, body: impl FnOnce(&mut Ui)) {
        Panel::vstack()
            .id_salt(self.label)
            .size((Sizing::fixed(self.w), Sizing::HUG))
            .gap(M.space.xs + M.space.xxs)
            .show(ui, |ui| {
                Text::new(self.label)
                    .style(&k.t.caption())
                    .size((Sizing::FILL, Sizing::fixed(CAPTION_H)))
                    .text_wrap(TextWrap::WrapWithOverflow)
                    .show(ui);
                let cell = Panel::zstack().size((Sizing::FILL, Sizing::fixed(self.h)));
                let cell = match self.surface {
                    Surface::Well => cell.padding(CELL_PADDING).chrome(k.t.card()),
                    Surface::LightWell => cell.padding(CELL_PADDING).chrome(light_well_chrome()),
                    Surface::Bare => cell,
                };
                cell.show(ui, body);
            });
    }
}
