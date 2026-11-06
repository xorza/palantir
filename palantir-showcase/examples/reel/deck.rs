//! [`Deck`]: the reel's one window, every slide a live subtree on one pan-zoom canvas.

use std::rc::Rc;
use std::time::Duration;

use palantir::glam::Vec2;
use palantir::widget::Shape;
use palantir::{
    Align, App, BackdropFilter, Block, Brush, Button, Checkbox, Chrome, ClipMode, ColorScheme,
    ComboBox, Configure, Corners, DockOperation, DockState, DockView, FontFamily, GpuView, Justify,
    Layer, Lens, LinearGradient, Metrics, Palette, Panel, Preference, RgbaF32, Scroll,
    ScrollbarTheme, Shadow, Sizing, Slider, Stroke, Switch, Text, TextEdit, TextSteps, Theme, Ui,
    UiPreferences, WidgetId, WindowToken, ZoomConfig, fmt,
};

use crate::cube::{Cube, CubeHandle};
use crate::dock::{self, Panes, Tab};
use crate::motion::{self, Point, View};
use crate::{code, cursor};

/// The logical size of a slide and of the window: 1280 × 720. The preview renders it at scale 1
/// and the final at 3, integer scales that put each logical pixel on whole physical ones, so no
/// hairline lands between two; a logical pixel is then 1.5 px at 1080p and 2 px at 1440p, where
/// stock text stays legible. Refused: logical 1920 × 1080, which needs scale 2/3 for the preview
/// and reads small at 1080p.
pub(crate) const SLIDE: Vec2 = Vec2::new(1280.0, 720.0);

/// The canvas between two slides.
const GAP: f32 = 160.0;

/// Slides a row of the canvas holds.
const COLUMNS: usize = 4;

/// The slides, in storyboard order.
pub(crate) const SLIDES: usize = 13;

/// A click's ring: it grows and fades over this long.
const RING: Duration = Duration::from_millis(400);

/// The deck's blend choices, which its combo binds by reference.
static BLENDS: [&str; 3] = ["Linear", "OKLab", "Okhsv"];

/// The canvas scroll's id, which the recorder moves the camera through.
pub(crate) fn canvas_id() -> WidgetId {
    WidgetId::from_hash("reel.canvas")
}

/// Where slide `k` sits on the canvas, its top-left corner.
pub(crate) fn slide_origin(k: usize) -> Vec2 {
    let (col, row) = (k % COLUMNS, k / COLUMNS);
    Vec2::new(col as f32, row as f32) * (SLIDE + GAP)
}

/// The camera's view of slide `k`, which fills the window.
pub(crate) fn slide_view(k: usize) -> View {
    let center = slide_origin(k) + SLIDE / 2.0;
    View {
        center: Point::new(f64::from(center.x), f64::from(center.y)),
        width: f64::from(SLIDE.x),
    }
}

/// The camera's view of the whole canvas, with a slide's gap of margin.
pub(crate) fn canvas_view() -> View {
    let rows = SLIDES.div_ceil(COLUMNS);
    let size = Vec2::new(COLUMNS as f32, rows as f32) * (SLIDE + GAP) - GAP;
    let width = (size.x + 2.0 * GAP).max((size.y + 2.0 * GAP) * SLIDE.x / SLIDE.y);
    View {
        center: Point::new(f64::from(size.x / 2.0), f64::from(size.y / 2.0)),
        width: f64::from(width),
    }
}

/// The board's frame around slide `k`, apart from [`id`]'s names, which belong to the slide.
fn frame_id(k: usize) -> WidgetId {
    WidgetId::from_hash(("reel.frame", k))
}

/// An id inside slide `k`.
pub(crate) fn id(k: usize, name: &str) -> WidgetId {
    WidgetId::from_hash(("reel.slide", k, name))
}

/// The id of the `i`-th of a run of `name`s inside slide `k`.
pub(crate) fn id_at(k: usize, name: &str, i: usize) -> WidgetId {
    WidgetId::from_hash(("reel.slide", k, name, i))
}

/// The roomy light theme slide 4 morphs to: the light palette, text 4 px larger, a third more
/// space, as a density preset moves both.
pub(crate) fn roomy_theme() -> Theme {
    let d = Metrics::DEFAULT;
    let scale = 4.0 / 3.0;
    let mut m = Metrics::DEFAULT;
    m.text = TextSteps {
        caption: d.text.caption + 4.0,
        body: d.text.body + 4.0,
        title: d.text.title + 4.0,
    };
    m.space.xxs *= scale;
    m.space.xs *= scale;
    m.space.sm *= scale;
    m.space.md *= scale;
    m.space.lg *= scale;
    m.space.xl *= scale;
    m.radius.control *= 2.0;
    m.radius.card *= 2.0;
    Theme::from_parts(&Palette::LIGHT, &m)
}

/// The reel's state between frames.
#[derive(Debug)]
pub(crate) struct Deck {
    /// The frame's time, which every time-driven picture reads; the wall clock never.
    pub(crate) now: Duration,
    /// When the slide on screen started.
    pub(crate) slide_start: Duration,
    /// Where the cursor is, in window px, and the last click.
    pub(crate) cursor: Vec2,
    pub(crate) click: Option<(Duration, Vec2)>,
    counter: u32,
    flash: Option<Duration>,
    switch: bool,
    slider: f64,
    combo: &'static &'static str,
    name: String,
    check: bool,
    dock: DockState<Tab>,
    operations: Vec<DockOperation<Tab>>,
    cube: CubeHandle,
}

impl Deck {
    pub(crate) fn new() -> Self {
        Self {
            now: Duration::ZERO,
            slide_start: Duration::ZERO,
            cursor: SLIDE / 2.0,
            click: None,
            counter: 0,
            flash: None,
            switch: false,
            slider: 0.25,
            combo: &BLENDS[0],
            name: String::new(),
            check: true,
            dock: dock::state(),
            operations: Vec::new(),
            cube: Cube::handle(),
        }
    }

    /// Seconds since the slide on screen started.
    const fn local(&self) -> f32 {
        self.now.saturating_sub(self.slide_start).as_secs_f32()
    }
}

impl App for Deck {
    fn record(&mut self, _win: WindowToken, ui: &mut Ui) {
        let ground = ui.theme().window_bg;
        let bars = hidden_bars(ui);
        Scroll::both()
            .id(canvas_id())
            .style(&bars)
            .zoom_config(ZoomConfig::new(0.05..=8.0, 1.1))
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                Panel::canvas().id_salt("reel.board").show(ui, |ui| {
                    for k in 0..SLIDES {
                        Panel::zstack()
                            .id(frame_id(k))
                            .at(slide_origin(k))
                            .size((Sizing::fixed(SLIDE.x), Sizing::fixed(SLIDE.y)))
                            .chrome(Chrome::rounded(ground, Corners::all(24.0)))
                            .clip(ClipMode::Rounded)
                            .show(ui, |ui| self.slide(k, ui));
                    }
                });
            });
        self.pointer(ui);
    }
}

/// Text colors that read on either palette.
fn ink(ui: &Ui) -> RgbaF32 {
    ui.theme().text.color
}

fn muted(ui: &Ui) -> RgbaF32 {
    ink(ui).with_alpha(0.62)
}

const ACCENT: RgbaF32 = RgbaF32::hex(0x6d8cff);

impl Deck {
    fn slide(&mut self, k: usize, ui: &mut Ui) {
        match k {
            0 => self.title(ui),
            1 => self.code(ui),
            2 => self.layout(ui),
            3 => self.widgets(ui),
            4 => self.morph(ui),
            5 => self.materials(ui),
            6 => self.crisp(ui),
            7 => self.docking(ui),
            8 => self.wgpu(ui),
            9 => self.driven(ui),
            10 => self.fast(ui),
            11 => self.made(ui),
            _ => self.outro(ui),
        }
    }

    /// A slide's heading over its body, centered.
    fn page(ui: &mut Ui, k: usize, heading: &str, body: impl FnOnce(&mut Ui)) {
        let color = ink(ui);
        Panel::vstack()
            .id(id(k, "page"))
            .size((Sizing::FILL, Sizing::FILL))
            .padding(56.0)
            .gap(28.0)
            .justify(Justify::Center)
            .show(ui, |ui| {
                Text::new(heading)
                    .font_size(44.0)
                    .bold()
                    .color(color)
                    .show(ui);
                body(ui);
            });
    }

    fn title(&self, ui: &mut Ui) {
        let (ink, muted) = (ink(ui), muted(ui));
        Panel::vstack()
            .id(id(0, "title"))
            .size((Sizing::FILL, Sizing::FILL))
            .gap(18.0)
            .child_align(Align::CENTER)
            .justify(Justify::Center)
            .show(ui, |ui| {
                Text::new("palantir")
                    .font_size(120.0)
                    .bold()
                    .color(ink)
                    .show(ui);
                Text::new("An immediate-mode GUI library for Rust")
                    .font_size(32.0)
                    .color(muted)
                    .show(ui);
            });
    }

    fn code(&mut self, ui: &mut Ui) {
        let flash = self.flash.map_or(0.0, |at| {
            let t = self.now.saturating_sub(at).as_secs_f32() / 0.9;
            (1.0 - t).max(0.0)
        });
        let counter = self.counter;
        let mut clicked = false;
        Self::page(ui, 1, "Code is the UI", |ui| {
            Panel::hstack().id(id(1, "row")).gap(40.0).show(ui, |ui| {
                code::listing(ui, flash);
                Panel::vstack()
                    .id(id(1, "window"))
                    .size((Sizing::fixed(320.0), Sizing::fixed(200.0)))
                    .padding(24.0)
                    .gap(16.0)
                    .chrome(window_chrome(ui))
                    .show(ui, |ui| {
                        Text::new(fmt!(ui, "clicks: {}", counter))
                            .font_size(24.0)
                            .show(ui);
                        clicked = Button::new()
                            .id(id(1, "button"))
                            .label("click me")
                            .show(ui)
                            .clicked();
                    });
            });
        });
        if clicked {
            self.counter += 1;
            self.flash = Some(self.now);
        }
    }

    fn layout(&self, ui: &mut Ui) {
        let t = self.local();
        // The window breathes between 520 and 1080 px over 6 s, from its narrowest.
        let width = 520.0 + 560.0 * (0.5 - 0.5 * (std::f32::consts::TAU * t / 6.0).cos());
        let muted = muted(ui);
        Self::page(ui, 2, "Two-pass layout", |ui| {
            Text::new("Hug columns take what their content asks; Fill columns share the rest.")
                .font_size(22.0)
                .color(muted)
                .show(ui);
            Panel::hstack()
                .id(id(2, "window"))
                .size((Sizing::fixed(width), Sizing::fixed(220.0)))
                .padding(16.0)
                .gap(12.0)
                .chrome(window_chrome(ui))
                .show(ui, |ui| {
                    column(ui, 2, "hug", "Hug", Sizing::HUG, 0x8b5cf6);
                    column(ui, 2, "fill-1", "Fill 1", Sizing::FILL, 0x3b82f6);
                    column(ui, 2, "hug-2", "Hug, longer", Sizing::HUG, 0x14b8a6);
                    column(ui, 2, "fill-2", "Fill 2", Sizing::fill(2.0), 0x22c55e);
                });
        });
    }

    fn widgets(&mut self, ui: &mut Ui) {
        let (switch, slider, combo, name) = (
            &mut self.switch,
            &mut self.slider,
            &mut self.combo,
            &mut self.name,
        );
        Self::page(ui, 3, "Widgets", |ui| {
            Panel::vstack()
                .id(id(3, "column"))
                .gap(24.0)
                .size((Sizing::fixed(520.0), Sizing::HUG))
                .show(ui, |ui| {
                    Switch::new(switch)
                        .id(id(3, "switch"))
                        .label("Live preview")
                        .show(ui);
                    Slider::new(slider, 0.0..=1.0)
                        .id(id(3, "slider"))
                        .size((Sizing::FILL, Sizing::HUG))
                        .show(ui);
                    ComboBox::new(combo, &BLENDS)
                        .id(id(3, "combo"))
                        .size((Sizing::fixed(220.0), Sizing::HUG))
                        .show(ui);
                    TextEdit::new(name)
                        .id(id(3, "edit"))
                        .size((Sizing::FILL, Sizing::HUG))
                        .show(ui);
                });
        });
    }

    fn morph(&mut self, ui: &mut Ui) {
        let check = &mut self.check;
        let muted = muted(ui);
        Self::page(ui, 4, "Theme morph", |ui| {
            Text::new(
                "Dark to light, compact to roomy: colors, margins and sizes move with real layout.",
            )
            .font_size(22.0)
            .color(muted)
            .show(ui);
            Panel::vstack()
                .id(id(4, "card"))
                .gap(14.0)
                .padding(24.0)
                .size((Sizing::fixed(560.0), Sizing::HUG))
                .chrome(window_chrome(ui))
                .show(ui, |ui| {
                    Text::new("Export").bold().show(ui);
                    Checkbox::new(check)
                        .id(id(4, "check"))
                        .label("Include metadata")
                        .show(ui);
                    Panel::hstack()
                        .id(id(4, "buttons"))
                        .gap(10.0)
                        .show(ui, |ui| {
                            Button::new().id(id(4, "cancel")).label("Cancel").show(ui);
                            Button::new().id(id(4, "export")).label("Export").show(ui);
                        });
                });
        });
    }

    fn materials(&self, ui: &mut Ui) {
        let t = self.local();
        let glass = t > 7.0;
        // The pane sweeps across the colors and back, 8 s a pass.
        let x = 140.0 + 520.0 * (0.5 - 0.5 * (std::f32::consts::TAU * t / 8.0).cos());
        Self::page(
            ui,
            5,
            if glass {
                "Refracting glass"
            } else {
                "Frosted glass"
            },
            |ui| {
                Panel::canvas()
                    .id(id(5, "stage"))
                    .size((Sizing::FILL, Sizing::FILL))
                    .show(ui, |ui| {
                        for (i, hex) in [0xf97316_u32, 0xec4899, 0x3b82f6, 0x22c55e, 0xfacc15]
                            .into_iter()
                            .enumerate()
                        {
                            Block::new()
                                .id(id_at(5, "blob", i))
                                .at((60.0 + 210.0 * i as f32, 40.0 + 90.0 * (i % 2) as f32))
                                .size(170.0)
                                .chrome(Chrome::rounded(RgbaF32::hex(hex), Corners::all(85.0)))
                                .show(ui);
                        }
                        let filter = if glass {
                            BackdropFilter::glass(Lens {
                                bezel: 28.0,
                                thickness: 40.0,
                                refractive_index: 1.5,
                                dispersion: 0.3,
                                highlight: 0.6,
                            })
                        } else {
                            BackdropFilter::frosted(18.0)
                        };
                        Block::new()
                            .id(id(5, "pane"))
                            .at((x, 70.0))
                            .size((Sizing::fixed(360.0), Sizing::fixed(220.0)))
                            .chrome(Chrome {
                                fill: RgbaF32::WHITE.with_alpha(0.12).into(),
                                border: Stroke::new(RgbaF32::WHITE.with_alpha(0.35), 1.0),
                                corners: Corners::all(32.0),
                                shadow: Shadow::NONE,
                                backdrop_filter: filter,
                            })
                            .show(ui);
                    });
            },
        );
    }

    fn crisp(&self, ui: &mut Ui) {
        let t = self.local();
        // Zooms in to 6× and back over 9 s.
        let zoom = 6.0_f32.powf(0.5 - 0.5 * (std::f32::consts::TAU * t / 9.0).cos());
        let inner = id(6, "zoom");
        let bars = hidden_bars(ui);
        Scroll::set_zoom(ui, inner, zoom);
        Scroll::set_offset(ui, inner, Vec2::new(300.0, 150.0) * (zoom - 1.0));
        Self::page(ui, 6, "Crisp at any scale", |ui| {
            Scroll::both()
                .id(inner)
                .zoom_config(ZoomConfig::new(1.0..=8.0, 1.1))
                .style(&bars)
                .size((Sizing::FILL, Sizing::FILL))
                .show(ui, |ui| {
                    Panel::canvas().id(id(6, "art")).show(ui, |ui| {
                        Block::new()
                            .id(id(6, "gradient"))
                            .at((40.0, 20.0))
                            .size((Sizing::fixed(320.0), Sizing::fixed(200.0)))
                            .chrome(Chrome {
                                fill: Brush::Linear(LinearGradient::two_stop(
                                    0.6,
                                    RgbaF32::hex(0x6d8cff),
                                    RgbaF32::hex(0xec4899),
                                )),
                                border: Stroke::NONE,
                                corners: Corners::all(28.0),
                                shadow: Shadow::drop(
                                    RgbaF32::BLACK.with_alpha(0.5),
                                    Vec2::new(0.0, 12.0),
                                    18.0,
                                ),
                                backdrop_filter: BackdropFilter::NONE,
                            })
                            .show(ui);
                        ui.add_shape(Shape::circle(
                            Vec2::new(520.0, 120.0),
                            70.0,
                            Stroke::new(RgbaF32::hex(0xfacc15), 6.0),
                        ));
                        Text::new("Aa")
                            .at((440.0, 220.0))
                            .font_size(72.0)
                            .bold()
                            .show(ui);
                    });
                });
        });
    }

    fn docking(&mut self, ui: &mut Ui) {
        let Self {
            dock, operations, ..
        } = self;
        Self::page(ui, 7, "Dock and windows", |ui| {
            Panel::vstack()
                .id(id(7, "host"))
                .size((Sizing::FILL, Sizing::FILL))
                .show(ui, |ui| {
                    operations.clear();
                    let dock_id = id(7, "dock");
                    DockView::scan(&mut ui.update_ctx(), dock_id, dock, operations);
                    for operation in operations.drain(..) {
                        dock.apply(operation);
                    }
                    DockView::new(dock_id, dock, operations)
                        .min_pane(160.0)
                        .show(ui, &mut Panes);
                    for operation in operations.drain(..) {
                        dock.apply(operation);
                    }
                });
        });
    }

    fn wgpu(&self, ui: &mut Ui) {
        self.cube.borrow_mut().angle = self.local() * 0.7;
        let cube = Rc::clone(&self.cube);
        let muted = muted(ui);
        Self::page(ui, 8, "Raw wgpu in a widget", |ui| {
            Panel::zstack()
                .id(id(8, "frame"))
                .size((Sizing::fixed(720.0), Sizing::fixed(400.0)))
                .chrome(Chrome::rounded(RgbaF32::hex(0x101320), Corners::all(40.0)))
                .clip(ClipMode::Rounded)
                .show(ui, |ui| {
                    GpuView::new(&cube)
                        .id(id(8, "view"))
                        .size((Sizing::FILL, Sizing::FILL))
                        .show(ui);
                    Panel::vstack()
                        .id(id(8, "label"))
                        .padding(20.0)
                        .show(ui, |ui| {
                            Text::new("GpuView: your pipeline, clipped and stacked")
                                .font_size(20.0)
                                .color(muted)
                                .show(ui);
                        });
                });
        });
    }

    fn driven(&self, ui: &mut Ui) {
        let t = self.local();
        let muted = muted(ui);
        Self::page(ui, 9, "Drive it like a person", |ui| {
            Text::new("click \"Export\"   ·   type \"report\"   ·   key Enter")
                .font_size(24.0)
                .family(FontFamily::MONO)
                .color(ACCENT)
                .show(ui);
            Panel::hstack().id(id(9, "row")).gap(24.0).show(ui, |ui| {
                for (i, (head, line)) in [
                    ("A headless test", "cargo test … ok"),
                    ("A live window", "Remote plays the same steps"),
                    ("An agent", "Darkroom serves its window over MCP"),
                ]
                .into_iter()
                .enumerate()
                {
                    let shown = (t - 0.8 * i as f32).clamp(0.0, 1.0);
                    Panel::vstack()
                        .id(id_at(9, "card", i))
                        .size((Sizing::fixed(340.0), Sizing::fixed(160.0)))
                        .padding(20.0)
                        .gap(10.0)
                        .chrome(window_chrome(ui))
                        .show(ui, |ui| {
                            Text::new(head).font_size(24.0).bold().show(ui);
                            Text::new(line)
                                .font_size(18.0)
                                .color(muted.with_alpha(muted.a * shown))
                                .show(ui);
                        });
                }
            });
        });
    }

    fn fast(&self, ui: &mut Ui) {
        let muted = muted(ui);
        Self::page(ui, 10, "Fast", |ui| {
            for (big, small) in [
                ("78 µs", "a frame of a 1,247-node screen (an Apple laptop)"),
                ("0", "allocations a frame"),
            ] {
                Panel::hstack()
                    .id_salt(big)
                    .gap(20.0)
                    .child_align(Align::CENTER)
                    .show(ui, |ui| {
                        Text::new(big).font_size(96.0).bold().color(ACCENT).show(ui);
                        Text::new(small).font_size(28.0).color(muted).show(ui);
                    });
            }
        });
    }

    fn made(&self, ui: &mut Ui) {
        let muted = muted(ui);
        Self::page(ui, 11, "Made with palantir", |ui| {
            Text::new("Every frame of this film was drawn by palantir, in Rust.")
                .font_size(28.0)
                .color(muted)
                .show(ui);
        });
    }

    fn outro(&self, ui: &mut Ui) {
        let (ink, muted) = (ink(ui), muted(ui));
        Panel::vstack()
            .id(id(12, "outro"))
            .size((Sizing::FILL, Sizing::FILL))
            .gap(22.0)
            .child_align(Align::CENTER)
            .justify(Justify::Center)
            .show(ui, |ui| {
                Text::new("palantir")
                    .font_size(96.0)
                    .bold()
                    .color(ink)
                    .show(ui);
                Text::new("cargo add palantir")
                    .font_size(32.0)
                    .family(FontFamily::MONO)
                    .color(ACCENT)
                    .show(ui);
                Text::new("github.com/xorza/palantir")
                    .font_size(24.0)
                    .color(muted)
                    .show(ui);
            });
    }

    /// The cursor and a click's ring, over everything and taking no input.
    fn pointer(&self, ui: &mut Ui) {
        let at = self.cursor;
        let ring = self.click.and_then(|(when, place)| {
            let t = self.now.checked_sub(when)?.as_secs_f64() / RING.as_secs_f64();
            (t < 1.0).then_some((place, t as f32))
        });
        ui.layer(Layer::Pointer).show(|ui| {
            Panel::canvas()
                .id_salt("reel.pointer")
                .size((Sizing::FILL, Sizing::FILL))
                .show(ui, |ui| {
                    if let Some((place, t)) = ring {
                        let grown = motion::min_jerk(f64::from(t)) as f32;
                        ui.add_shape(Shape::circle(
                            place,
                            6.0 + 20.0 * grown,
                            Stroke::new(ACCENT.with_alpha(0.7 * (1.0 - t)), 2.5),
                        ));
                    }
                    cursor::draw(ui, at);
                });
        });
    }
}

/// Scroll bars that paint nothing: a camera moves the canvas, not a person.
fn hidden_bars(ui: &Ui) -> ScrollbarTheme {
    ScrollbarTheme {
        track: RgbaF32::TRANSPARENT,
        thumb: RgbaF32::TRANSPARENT,
        thumb_hovered: RgbaF32::TRANSPARENT,
        thumb_active: RgbaF32::TRANSPARENT,
        ..ui.theme().scrollbar.clone()
    }
}

/// A window drawn inside a slide: its fill a step off the ground, a hairline and a soft shadow.
fn window_chrome(ui: &Ui) -> Chrome {
    let theme = ui.theme();
    Chrome {
        fill: theme.button.unselected.normal.chrome.fill.clone(),
        border: Stroke::new(theme.text.color.with_alpha(0.12), 1.0),
        corners: Corners::all(14.0),
        shadow: Shadow::drop(RgbaF32::BLACK.with_alpha(0.35), Vec2::new(0.0, 10.0), 16.0),
        backdrop_filter: BackdropFilter::NONE,
    }
}

/// One column of slide 2's window: a block sized by `sizing`, its label naming the rule.
fn column(ui: &mut Ui, k: usize, name: &str, label: &str, sizing: Sizing, hex: u32) {
    Panel::vstack()
        .id(id(k, name))
        .size((sizing, Sizing::FILL))
        .padding(12.0)
        .chrome(Chrome::rounded(RgbaF32::hex(hex), Corners::all(10.0)))
        .show(ui, |ui| {
            Text::new(label)
                .font_size(20.0)
                .color(RgbaF32::WHITE)
                .show(ui);
        });
}

/// The window as the reel starts: the stock dark theme, the scheme held there.
pub(crate) fn dress(ui: &mut Ui) {
    ui.set_preferences(UiPreferences {
        scheme: Preference::Fixed(ColorScheme::Dark),
        ..ui.preferences()
    });
    ui.set_theme(ColorScheme::Dark, Theme::stock(ColorScheme::Dark));
}
