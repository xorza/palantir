//! Shared workload for the frame and allocation benches and the `frame_visual`
//! example: a synthetic but *designed* app screen — a dark telemetry
//! console — rather than a pile of widgets. It exercises every public
//! layout driver (HStack/VStack/ZStack/Canvas/Grid/WrapHStack/WrapVStack
//! and Scroll on **both** axes), every non-animated public widget
//! (Panel/Frame/Button/Text/Grid/Scroll/Checkbox/RadioButton/Switch/
//! Slider/DragValue/ComboBox/ProgressBar/Separator/TextEdit/Tooltip/Popup),
//! every authoring shape family (Rect / Triangle / Curve / Polyline / Mesh /
//! Shadow / Text), every `Brush` variant (Solid / Linear / Radial / Conic)
//! at both chrome and shape level, chrome drop shadows, grid cell spans,
//! `disabled` / `hidden` cascade flattening, and the popup/tooltip layers.
//!
//! **Nothing animated belongs in here.** `Spinner` — and any `PaintAnim` —
//! wakes the host every frame by design, so `frame/cached_*` could never
//! settle to `Damage::Skip` and `frame/partial_*` would grow past the single
//! footer-counter rect both arms exist to measure. `Modal`, `ContextMenu`,
//! and `GpuView` are absent for the same class of reason: the first two
//! record nothing until an interaction the benches never deliver, and
//! `GpuView` needs a `wgpu::Device` the deviceless CPU/alloc harnesses
//! don't have.

use std::f32::consts::PI;
use std::time::Duration;

use crate::layout::types::align::Align;
use crate::layout::types::justify::Justify;
use crate::layout::types::sizing::Sizing;
use crate::layout::types::track::Track;
use crate::primitives::background::Background;
use crate::primitives::brush::Brush;
use crate::primitives::brush::gradient::conic::ConicGradient;
use crate::primitives::brush::gradient::linear::LinearGradient;
use crate::primitives::brush::gradient::radial::RadialGradient;
use crate::primitives::brush::gradient::stops::Stop;
use crate::primitives::color::{Color, ColorU8};
use crate::primitives::corners::Corners;
use crate::primitives::mesh::Mesh;
use crate::primitives::rect::Rect;
use crate::primitives::shadow::Shadow;
use crate::primitives::stroke::Stroke;
use crate::primitives::transform::TranslateScale;
use crate::scene::node::Configure;
use crate::scene::visibility::Visibility;
use crate::shape::Shape;
use crate::shape::polyline::PolylineColors;
use crate::shape::style::{LineCap, LineJoin};
use crate::text::wrap::TextWrap;
use crate::ui::Ui;
use crate::widgets::button::Button;
use crate::widgets::checkbox::Checkbox;
use crate::widgets::combo_box::ComboBox;
use crate::widgets::drag_value::DragValue;
use crate::widgets::frame::Frame;
use crate::widgets::grid::Grid;
use crate::widgets::panel::Panel;
use crate::widgets::popup::Popup;
use crate::widgets::progress_bar::ProgressBar;
use crate::widgets::radio::RadioButton;
use crate::widgets::scroll::Scroll;
use crate::widgets::separator::Separator;
use crate::widgets::slider::Slider;
use crate::widgets::switch::Switch;
use crate::widgets::text::Text;
use crate::widgets::text_edit::TextEdit;
use crate::widgets::theme::text_style::TextStyle;
use crate::widgets::tooltip::Tooltip;

pub(crate) const BENCH_SCALE: usize = 32;

const APP_BG: Color = Color::hex(0x0f1116);
const CARD_BG: Color = Color::hex(0x1a1d25);
const WELL_BG: Color = Color::hex(0x13151b);
const BORDER: Color = Color::hex(0x2b303d);
const ACCENT: Color = Color::hex(0x4cd3ff);
const WARN: Color = Color::hex(0xffa63d);
const OK: Color = Color::hex(0xd9ff57);
const VIOLET: Color = Color::hex(0xd897ff);
const TEXT_DIM: Color = Color::hex(0x8b93a7);

/// Raised card: fill + hairline border + a real chrome drop shadow. The
/// shadow is the only thing in the fixture that drives the chrome branch
/// of `emit_shadow`, so keep it non-noop.
fn card_bg() -> Background {
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
fn well_bg() -> Background {
    Background {
        fill: WELL_BG.into(),
        corners: Corners::all(6.0),
        ..Default::default()
    }
}

fn section_style() -> TextStyle {
    TextStyle::default()
        .with_font_size(11.0)
        .with_color(TEXT_DIM)
        .bold()
}

fn caption_style() -> TextStyle {
    TextStyle::default()
        .with_font_size(11.0)
        .with_color(TEXT_DIM)
}

fn body_style() -> TextStyle {
    TextStyle::default().with_font_size(13.0)
}

/// Titled card: section caption over `body`, on [`card_bg`]. `h` lets a
/// card either hug its content or claim the column's leftover height
/// (the activity list is the only `FILL` one).
fn card(ui: &mut Ui, id: &'static str, title: &'static str, h: Sizing, body: impl FnOnce(&mut Ui)) {
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

/// Persistent state for widgets that mutate user data (TextEdit needs
/// a `&mut String`, Checkbox a `&mut bool`, RadioButton a `&mut T`).
///
/// `tick` drives the footer-status counter and is the **only** field
/// the partial-damage arm mutates between iterations. The footer Text
/// node is sized `Fixed(120.0)` so the changing digits don't shift
/// sibling layout — the damage rect collapses to that single node's
/// arranged box.
#[derive(Debug)]
pub struct FrameFixture {
    name: String,
    notes: String,
    enabled: bool,
    role: u8,
    pub(crate) tick: u32,
    /// Post-arrange translate applied to the main content panel. Used
    /// by the `frame/scrolling_cpu` bench arm to model continuous
    /// position change WITHOUT changing layout — the cascade walks the
    /// full subtree but layout/measure cache hits trivially. Tests
    /// whether a cascade delta-cache (cached output translated by
    /// `parent_transform`) would meaningfully reduce cascade cost.
    pub(crate) scroll_offset: glam::Vec2,
    /// Backing values for the settings grid (Slider / DragValue /
    /// ComboBox / Switch). Held constant across bench iterations —
    /// only `tick` mutates — so they never perturb the steady-state
    /// damage `Skip` / `Partial` invariants the arms assert; they widen
    /// widget coverage only. Seeded to mid-range values so the visual
    /// harness shows them in a representative, non-empty state.
    volume: f32,
    mix: f32,
    zoom: f64,
    quality: usize,
    dark_mode: bool,
    grid_rows: Vec<Track>,
}

impl Default for FrameFixture {
    fn default() -> Self {
        Self {
            name: String::new(),
            notes: String::new(),
            enabled: true,
            role: 1,
            tick: 0,
            scroll_offset: glam::Vec2::ZERO,
            volume: 0.65,
            mix: 0.35,
            zoom: 42.0_f64,
            quality: 2,
            dark_mode: true,
            grid_rows: Vec::new(),
        }
    }
}

impl FrameFixture {
    pub fn render(&mut self, scale: usize, ui: &mut Ui) {
        build_ui(self, scale, ui);
    }
}

pub(crate) fn build_ui(state: &mut FrameFixture, scale: usize, ui: &mut Ui) {
    let sidebar_items = 5 * scale;
    let chat_messages = 2 * scale;
    let film_cells = 2 * scale;
    let prop_rows = 4 + scale;
    let tag_count = 3 * scale;
    let badge_count = scale;
    state.grid_rows.resize(prop_rows, Track::hug());

    Panel::vstack()
        .gap(10.0)
        .padding(12.0)
        .size((Sizing::FILL, Sizing::FILL))
        .background(Background {
            fill: APP_BG.into(),
            ..Default::default()
        })
        .show(ui, |ui| {
            app_bar(ui);

            Panel::hstack()
                .id_salt("body")
                .gap(12.0)
                .size((Sizing::FILL, Sizing::FILL))
                .transform(TranslateScale::from_translation(state.scroll_offset))
                .show(ui, |ui| {
                    sidebar(ui, sidebar_items);

                    // Page scroll, not a bare VStack: the card column is taller
                    // than a normal window, and an overflowing column paints
                    // over the status bar — which occludes the footer counter
                    // and collapses the `frame/partial_*` arms to `Damage::Skip`.
                    // Clipping the overflow here keeps the counter visible at
                    // every viewport size. Every child must therefore be Hug or
                    // Fixed: a scroll passes ∞ on its main axis, so a `Fill`
                    // child would resolve against nothing.
                    Scroll::vertical()
                        .id_salt("page-scroll")
                        .gap(10.0)
                        .size((Sizing::FILL, Sizing::FILL))
                        .show(ui, |ui| {
                            // Ordered diverse-first: the visually varied cards lead so
                            // they fill the `frame_visual` viewport, while the bulky
                            // repetitive lists (properties, tags) trail.
                            stat_strip(ui);
                            request_card(state, ui);
                            settings_card(state, ui);
                            specimen_card(ui);
                            filmstrip(ui, film_cells);
                            activity_card(ui, chat_messages);
                            properties_card(state, ui, prop_rows);
                            tags_card(ui, tag_count, badge_count);
                            notes_card(state, ui);
                        });
                });

            status_bar(state, ui);
        });
}

fn app_bar(ui: &mut Ui) {
    Panel::hstack()
        .id_salt("app-bar")
        .gap(10.0)
        .size((Sizing::FILL, Sizing::HUG))
        .child_align(Align::CENTER)
        .show(ui, |ui| {
            // Brand dot: the only conic-gradient chrome fill in the tree.
            Frame::new()
                .id_salt("brand")
                .size((Sizing::fixed(22.0), Sizing::fixed(22.0)))
                .background(Background {
                    fill: Brush::Conic(ConicGradient::new(
                        glam::Vec2::splat(0.5),
                        0.0,
                        [
                            Stop::new(0.0, ColorU8::hex(0x4cd3ff)),
                            Stop::new(0.5, ColorU8::hex(0xd897ff)),
                            Stop::new(1.0, ColorU8::hex(0x4cd3ff)),
                        ],
                    )),
                    corners: Corners::all(11.0),
                    ..Default::default()
                })
                .show(ui);
            Text::new("Palantir")
                .id_salt("title")
                .style(&TextStyle::default().with_font_size(19.0).bold())
                .show(ui);
            Text::new("frame bench")
                .id_salt("subtitle")
                .style(&caption_style())
                .show(ui);
            Frame::new()
                .id_salt("title-spacer")
                .size((Sizing::FILL, Sizing::fixed(1.0)))
                .show(ui);
            for i in 0..5 {
                let label = ui.fmt(format_args!("Action {i}"));
                let btn = Button::new()
                    .id_salt(("hdr", i))
                    .label(label)
                    .show(ui)
                    .snapshot();
                Tooltip::on(&btn)
                    .text("Header action")
                    .delay(Duration::ZERO)
                    .show(ui);
            }
            // Cascade `disabled` flattening — and a real UI state, not a
            // marker: the deploy action is unavailable until a run finishes.
            Button::new()
                .id_salt("deploy")
                .label("Deploy")
                .disabled(true)
                .show(ui);
        });
}

fn sidebar(ui: &mut Ui, items: usize) {
    Panel::vstack()
        .id_salt("sidebar")
        .gap(6.0)
        .padding(8.0)
        .size((Sizing::fixed(216.0), Sizing::FILL))
        .background(card_bg())
        .clip_rounded()
        .show(ui, |ui| {
            Text::new("WORKSPACE")
                .id_salt("sb-title")
                .style(&section_style())
                .show(ui);
            Scroll::vertical()
                .id_salt("sidebar-scroll")
                .gap(3.0)
                .size((Sizing::FILL, Sizing::FILL))
                .show(ui, |ui| {
                    for i in 0..items {
                        // Every 8th row is a group caption, so the long list
                        // is a grouped nav tree rather than N identical
                        // buttons — and the scroll viewport measures two
                        // different node shapes instead of one.
                        if i % 8 == 0 {
                            let head = ui.fmt(format_args!("GROUP {}", i / 8));
                            Text::new(head)
                                .id_salt(("side-group", i))
                                .style(&caption_style())
                                .show(ui);
                        } else {
                            let label = ui.fmt(format_args!("Sidebar item {i}"));
                            Button::new()
                                .id_salt(("side", i))
                                .label(label)
                                .size((Sizing::FILL, Sizing::HUG))
                                .show(ui);
                        }
                    }
                });
            Separator::horizontal().id_salt("sb-divider").show(ui);
            Panel::hstack()
                .id_salt("sb-foot-row")
                .gap(4.0)
                .justify(Justify::Center)
                .size((Sizing::FILL, Sizing::HUG))
                .show(ui, |ui| {
                    for i in 0..3 {
                        Button::new()
                            .id_salt(("sb-foot", i))
                            .label(ui.fmt(format_args!("F{i}")))
                            .show(ui);
                    }
                });
        });
}

/// Four metric tiles. Each is a ZStack — gradient plate under a text
/// column — so the strip carries all four `Brush` variants at once.
fn stat_strip(ui: &mut Ui) {
    const STATS: [(&str, &str, &str); 4] = [
        ("FRAMES", "148 920", "+12.4%"),
        ("NODES", "8 412", "+1.8%"),
        ("DRAW CALLS", "36", "-4"),
        ("GPU", "2.81 ms", "steady"),
    ];
    const DELTA: [Color; 4] = [OK, ACCENT, OK, TEXT_DIM];

    Panel::hstack()
        .id_salt("stats")
        .gap(10.0)
        .size((Sizing::FILL, Sizing::HUG))
        .show(ui, |ui| {
            for (i, (label, value, delta)) in STATS.iter().enumerate() {
                let fill = match i {
                    0 => Brush::Linear(LinearGradient::two_stop(
                        0.6,
                        ColorU8::hex(0x1d2440),
                        ColorU8::hex(0x2b3a63),
                    )),
                    1 => Brush::Radial(RadialGradient::two_stop_centered(
                        ColorU8::hex(0x2a2350),
                        ColorU8::hex(0x171a2b),
                    )),
                    2 => Brush::Conic(ConicGradient::new(
                        glam::Vec2::new(0.15, 0.9),
                        0.0,
                        [
                            Stop::new(0.0, ColorU8::hex(0x1b2b2e)),
                            Stop::new(0.55, ColorU8::hex(0x24404a)),
                            Stop::new(1.0, ColorU8::hex(0x1b2b2e)),
                        ],
                    )),
                    _ => Brush::Solid(Color::hex(0x232734)),
                };
                Panel::zstack()
                    .id_salt(("stat", i))
                    .size((Sizing::fill(1.0), Sizing::fixed(74.0)))
                    .show(ui, |ui| {
                        Frame::new()
                            .id_salt(("stat-plate", i))
                            .size((Sizing::FILL, Sizing::FILL))
                            .background(Background {
                                fill,
                                stroke: Stroke::solid(BORDER, 1.0),
                                corners: Corners::all(8.0),
                                shadow: Shadow::drop(
                                    Color::rgba(0.0, 0.0, 0.0, 0.45),
                                    glam::Vec2::new(0.0, 2.0),
                                    8.0,
                                ),
                            })
                            .show(ui);
                        // Cascade `Hidden` flattening — the alert ring this
                        // tile would show on a threshold breach. A ZStack
                        // sibling, so reserving its box costs no layout.
                        Frame::new()
                            .id_salt(("stat-alert", i))
                            .size((Sizing::FILL, Sizing::FILL))
                            .background(Background {
                                stroke: Stroke::solid(WARN, 2.0),
                                corners: Corners::all(8.0),
                                ..Default::default()
                            })
                            .visibility(Visibility::Hidden)
                            .show(ui);
                        Panel::vstack()
                            .id_salt(("stat-text", i))
                            .gap(2.0)
                            .padding(10.0)
                            .size((Sizing::FILL, Sizing::FILL))
                            .show(ui, |ui| {
                                Text::new(*label)
                                    .id_salt(("stat-l", i))
                                    .style(&caption_style())
                                    .show(ui);
                                Text::new(*value)
                                    .id_salt(("stat-v", i))
                                    .style(&TextStyle::default().with_font_size(22.0).bold())
                                    .show(ui);
                                Text::new(*delta)
                                    .id_salt(("stat-d", i))
                                    .style(
                                        &TextStyle::default()
                                            .with_font_size(11.0)
                                            .with_color(DELTA[i]),
                                    )
                                    .show(ui);
                            });
                    });
            }
        });
}

fn request_card(state: &mut FrameFixture, ui: &mut Ui) {
    card(ui, "request", "REQUEST", Sizing::HUG, |ui| {
        Panel::hstack()
            .id_salt("form-row")
            .gap(8.0)
            .child_align(Align::CENTER)
            .size((Sizing::FILL, Sizing::HUG))
            .show(ui, |ui| {
                TextEdit::new(&mut state.name)
                    .id_salt("edit-name")
                    .size((Sizing::fill(2.0), Sizing::HUG))
                    .show(ui);
                Checkbox::new(&mut state.enabled)
                    .id_salt("enabled")
                    .label("enabled")
                    .show(ui);
                for v in 0u8..3 {
                    RadioButton::new(&mut state.role, v)
                        .id_salt(("role", v))
                        .label(["read", "write", "admin"][v as usize])
                        .show(ui);
                }
                Button::new().id_salt("submit").label("Submit").show(ui);
            });
        // `Collapsed`, not `Hidden`: the validation line a real form keeps
        // recorded but out of the way while the input is valid. `Hidden`
        // would still reserve its row and leave a gap under the controls;
        // the paint-skip path it covers is exercised in [`stat_strip`],
        // where a ZStack sibling can hide without moving anything.
        Text::new("Name is required")
            .id_salt("form-error")
            .style(&caption_style().with_color(WARN))
            .visibility(Visibility::Collapsed)
            .show(ui);
    });
}

/// Settings as a two-column `Grid` (label | control) rather than loose
/// rows, so the controls align on a real track edge — and so the fixture
/// carries a second, statically-sized Grid alongside the dynamic one in
/// [`properties_card`].
fn settings_card(state: &mut FrameFixture, ui: &mut Ui) {
    card(ui, "settings", "SETTINGS", Sizing::HUG, |ui| {
        let rows = [Track::hug(); 6];
        Grid::new()
            .id_salt("settings-grid")
            .cols([Track::hug().min(92.0), Track::fill()])
            .rows(rows)
            .gap_xy(8.0, 12.0)
            .size((Sizing::FILL, Sizing::HUG))
            .show(ui, |ui| {
                Text::new("Appearance")
                    .id_salt("s-l0")
                    .style(&body_style())
                    .grid_cell((0, 0))
                    .show(ui);
                Panel::hstack()
                    .id_salt("s-c0")
                    .gap(10.0)
                    .child_align(Align::CENTER)
                    .size((Sizing::FILL, Sizing::HUG))
                    .grid_cell((0, 1))
                    .show(ui, |ui| {
                        Switch::new(&mut state.dark_mode)
                            .id_salt("dark-mode")
                            .label("dark mode")
                            .show(ui);
                        Separator::vertical().id_salt("s-vsep").show(ui);
                        let quality_opts = ["Low", "Medium", "High", "Ultra"];
                        ComboBox::new(&mut state.quality, &quality_opts)
                            .id_salt("quality")
                            .size((Sizing::fixed(140.0), Sizing::HUG))
                            .show(ui);
                    });

                // Full-width rule, drawn with a spanning cell so the grid
                // carries `grid_span` coverage in its simplest honest form.
                Frame::new()
                    .id_salt("s-rule")
                    .size((Sizing::FILL, Sizing::fixed(1.0)))
                    .background(Background {
                        fill: BORDER.into(),
                        ..Default::default()
                    })
                    .grid_cell((1, 0))
                    .grid_span((1, 2))
                    .show(ui);

                Text::new("Zoom")
                    .id_salt("s-l2")
                    .style(&body_style())
                    .grid_cell((2, 0))
                    .show(ui);
                DragValue::new(&mut state.zoom)
                    .id_salt("zoom")
                    .speed(0.5)
                    .range(0.0..=100.0)
                    .decimals(0)
                    .suffix("%")
                    .size((Sizing::fixed(90.0), Sizing::HUG))
                    .grid_cell((2, 1))
                    .show(ui);

                Text::new("Volume")
                    .id_salt("s-l3")
                    .style(&body_style())
                    .grid_cell((3, 0))
                    .show(ui);
                Slider::new(&mut state.volume, 0.0..=1.0)
                    .id_salt("volume")
                    .step(0.05)
                    .grid_cell((3, 1))
                    .show(ui);

                Text::new("Wet / dry")
                    .id_salt("s-l4")
                    .style(&body_style())
                    .grid_cell((4, 0))
                    .show(ui);
                Slider::new(&mut state.mix, 0.0..=1.0)
                    .id_salt("mix")
                    .grid_cell((4, 1))
                    .show(ui);

                Text::new("Indexing")
                    .id_salt("s-l5")
                    .style(&body_style())
                    .grid_cell((5, 0))
                    .show(ui);
                ProgressBar::new(0.62)
                    .id_salt("progress")
                    .grid_cell((5, 1))
                    .show(ui);
            });
    });
}

/// Specimen sheet: one captioned canvas per shape family, tiled through a
/// `WrapHStack` so the sheet reflows to whatever width the column has
/// instead of clumping in a corner.
fn specimen_card(ui: &mut Ui) {
    card(ui, "specimens", "SHAPES", Sizing::HUG, |ui| {
        Panel::wrap_hstack()
            .id_salt("specimen-wrap")
            .gap(8.0)
            .size((Sizing::FILL, Sizing::HUG))
            .show(ui, |ui| {
                specimen(ui, "brushes", "brushes", add_brush_swatches);
                specimen(ui, "curves", "curves", add_curves);
                specimen(ui, "polyline", "polyline", add_polyline);
                specimen(ui, "arcs", "arc / circle", add_arcs);
                specimen(ui, "solids", "triangle / mesh", add_solids);
                specimen(ui, "shadow", "shadow", add_shadow);
            });
    });
}

/// One 168x96 specimen cell: caption over a recessed canvas. Shapes the
/// body adds are canvas-local, so every cell shares one coordinate box.
fn specimen(ui: &mut Ui, id: &'static str, label: &'static str, body: impl FnOnce(&mut Ui)) {
    Panel::vstack()
        .id_salt(("spec", id))
        .gap(4.0)
        .size((Sizing::fixed(168.0), Sizing::HUG))
        .show(ui, |ui| {
            Text::new(label)
                .id_salt(("spec-cap", id))
                .style(&caption_style())
                .show(ui);
            Panel::canvas()
                .id_salt(("spec-canvas", id))
                .size((Sizing::FILL, Sizing::fixed(96.0)))
                .background(well_bg())
                .show(ui, body);
        });
}

/// Shape-level (not chrome-level) Solid / Linear / Radial / Conic fills.
fn add_brush_swatches(ui: &mut Ui) {
    ui.add_shape(
        Shape::rect(Rect::new(6.0, 6.0, 74.0, 38.0))
            .corners(6.0)
            .fill(Color::hex(0xd9544c))
            .stroke(Stroke::solid(Color::rgba(1.0, 1.0, 1.0, 0.5), 1.0)),
    );
    ui.add_shape(
        Shape::rect(Rect::new(88.0, 6.0, 74.0, 38.0))
            .corners(6.0)
            .fill(
                LinearGradient::builder(PI / 2.0)
                    .stop(0.0, ColorU8::hex(0x1a1a2e))
                    .stop(1.0, ColorU8::hex(0x4c5cdb)),
            ),
    );
    ui.add_shape(
        Shape::rect(Rect::new(6.0, 52.0, 74.0, 38.0))
            .corners(6.0)
            .fill(RadialGradient::two_stop_centered(
                ColorU8::hex(0xfacc15),
                ColorU8::hex(0x1a1a2e),
            )),
    );
    ui.add_shape(
        Shape::rect(Rect::new(88.0, 52.0, 74.0, 38.0))
            .corners(6.0)
            .fill(ConicGradient::new(
                glam::Vec2::splat(0.5),
                0.0,
                [
                    Stop::new(0.0, ColorU8::hex(0xff5e44)),
                    Stop::new(0.5, ColorU8::hex(0x46c46c)),
                    Stop::new(1.0, ColorU8::hex(0x4c5cdb)),
                ],
            )),
    );
}

fn add_curves(ui: &mut Ui) {
    ui.add_shape(
        Shape::line(
            glam::Vec2::new(12.0, 16.0),
            glam::Vec2::new(156.0, 28.0),
            3.0,
        )
        .brush(ACCENT)
        .cap(LineCap::Round),
    );
    ui.add_shape(
        Shape::quadratic_bezier(
            glam::Vec2::new(12.0, 50.0),
            glam::Vec2::new(84.0, 22.0),
            glam::Vec2::new(156.0, 50.0),
            3.0,
        )
        .brush(WARN)
        .cap(LineCap::Square),
    );
    ui.add_shape(
        Shape::cubic_bezier(
            glam::Vec2::new(12.0, 86.0),
            glam::Vec2::new(56.0, 56.0),
            glam::Vec2::new(112.0, 96.0),
            glam::Vec2::new(156.0, 68.0),
            4.0,
        )
        .brush(OK)
        .cap(LineCap::Round),
    );
}

fn add_polyline(ui: &mut Ui) {
    let pts: [glam::Vec2; 6] = [
        glam::Vec2::new(14.0, 74.0),
        glam::Vec2::new(42.0, 32.0),
        glam::Vec2::new(70.0, 66.0),
        glam::Vec2::new(98.0, 26.0),
        glam::Vec2::new(126.0, 62.0),
        glam::Vec2::new(154.0, 38.0),
    ];
    let cols = [
        Color::hex(0xff5e44),
        Color::hex(0xffa63d),
        Color::hex(0xd9ff57),
        Color::hex(0x46c46c),
        Color::hex(0x4cd3ff),
        Color::hex(0xd897ff),
    ];
    ui.add_shape(Shape::polyline(&pts, PolylineColors::PerPoint(&cols), 4.0).join(LineJoin::Round));
}

fn add_arcs(ui: &mut Ui) {
    ui.add_shape(
        Shape::arc(glam::Vec2::new(84.0, 78.0), 40.0, PI, PI, 5.0)
            .brush(ACCENT)
            .cap(LineCap::Round),
    );
    ui.add_shape(Shape::circle(glam::Vec2::new(84.0, 26.0), 12.0, 3.0).brush(VIOLET));
}

fn add_solids(ui: &mut Ui) {
    ui.add_shape(
        Shape::triangle(
            glam::Vec2::new(18.0, 82.0),
            glam::Vec2::new(52.0, 20.0),
            glam::Vec2::new(86.0, 82.0),
        )
        .fill(WARN)
        .radius(2.0_f32),
    );
    ui.add_shape(Shape::mesh(gradient_mesh()));
}

/// A `Shape::shadow` under the rect that casts it — the shape-level peer
/// of the chrome shadow on [`card_bg`].
fn add_shadow(ui: &mut Ui) {
    let plate = Rect::new(34.0, 20.0, 100.0, 52.0);
    ui.add_shape(
        Shape::shadow(Shadow::drop(
            Color::rgba(0.0, 0.0, 0.0, 0.75),
            glam::Vec2::new(0.0, 7.0),
            9.0,
        ))
        .at(plate)
        .corners(10.0),
    );
    ui.add_shape(
        Shape::rect(plate)
            .corners(10.0)
            .fill(Color::hex(0x30364a))
            .stroke(Stroke::solid(BORDER, 1.0)),
    );
}

/// The mesh payload is built once and leaked: `Shape::mesh` borrows a
/// `&'static Mesh`, and rebuilding it per frame would allocate — which the
/// `alloc_free` gate forbids.
fn gradient_mesh() -> &'static Mesh {
    use std::sync::OnceLock;
    static MESH_PTR: OnceLock<usize> = OnceLock::new();
    unsafe {
        &*(*MESH_PTR.get_or_init(|| {
            let mut m = Mesh::new();
            let a = m.vertex(glam::Vec2::new(96.0, 82.0), ColorU8::hex(0xff5e44));
            let b = m.vertex(glam::Vec2::new(128.0, 22.0), ColorU8::hex(0xfacc15));
            let c = m.vertex(glam::Vec2::new(160.0, 82.0), ColorU8::hex(0x46c46c));
            m.triangle(a, b, c);
            Box::into_raw(Box::new(m)) as usize
        }) as *const Mesh)
    }
}

/// Horizontally-scrolled strip — the fixture's only `Scroll::horizontal`,
/// so both scroll axes are exercised in one tree.
fn filmstrip(ui: &mut Ui, cells: usize) {
    card(ui, "recent", "RECENT CAPTURES", Sizing::HUG, |ui| {
        Scroll::horizontal()
            .id_salt("film-scroll")
            .gap(8.0)
            .padding(6.0)
            .size((Sizing::FILL, Sizing::fixed(74.0)))
            .background(well_bg())
            .show(ui, |ui| {
                for i in 0..cells {
                    Panel::vstack()
                        .id_salt(("film", i))
                        .gap(3.0)
                        .size((Sizing::fixed(84.0), Sizing::FILL))
                        .show(ui, |ui| {
                            Frame::new()
                                .id_salt(("film-thumb", i))
                                .size((Sizing::FILL, Sizing::FILL))
                                .background(Background {
                                    fill: Brush::Linear(LinearGradient::two_stop(
                                        0.9,
                                        ColorU8::hex(0x24304d),
                                        ColorU8::hex(0x3d2a52),
                                    )),
                                    corners: Corners::all(5.0),
                                    ..Default::default()
                                })
                                .show(ui);
                            Text::new(ui.fmt(format_args!("shot_{i:03}")))
                                .id_salt(("film-cap", i))
                                .style(&caption_style())
                                .show(ui);
                        });
                }
            });
    });
}

/// Fixed height, not `Fill`: this card lives inside the page scroll, whose
/// main axis is unbounded, so a `Fill` height would resolve against nothing
/// and the inner scroll would grow to its full content instead of paging.
fn activity_card(ui: &mut Ui, messages: usize) {
    card(ui, "activity", "ACTIVITY", Sizing::fixed(268.0), |ui| {
        Scroll::vertical()
            .id_salt("chat-scroll")
            .gap(8.0)
            .padding(4.0)
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                for i in 0..messages {
                    Panel::hstack()
                        .id_salt(("chat-row", i))
                        .gap(8.0)
                        .size((Sizing::FILL, Sizing::HUG))
                        .show(ui, |ui| {
                            Frame::new()
                                .id_salt(("avatar", i))
                                .size((Sizing::fixed(34.0), Sizing::fixed(34.0)))
                                .background(Background {
                                    fill: Brush::Radial(RadialGradient::two_stop_centered(
                                        ColorU8::hex(0xfacc15),
                                        ColorU8::hex(0x4c5cdb),
                                    )),
                                    corners: Corners::all(17.0),
                                    ..Default::default()
                                })
                                .show(ui);
                            Panel::vstack()
                                .id_salt(("chat-text", i))
                                .gap(2.0)
                                .size((Sizing::FILL, Sizing::HUG))
                                .show(ui, |ui| {
                                    Panel::hstack()
                                        .id_salt(("chat-meta", i))
                                        .gap(6.0)
                                        .child_align(Align::CENTER)
                                        .size((Sizing::FILL, Sizing::HUG))
                                        .show(ui, |ui| {
                                            let name = ui.fmt(format_args!("user_{i}"));
                                            Text::new(name)
                                                .id_salt(("from", i))
                                                .style(
                                                    &TextStyle::default()
                                                        .with_font_size(12.0)
                                                        .bold(),
                                                )
                                                .show(ui);
                                            let at = ui.fmt(format_args!("{:02}:{:02}", i % 24, i));
                                            Text::new(at)
                                                .id_salt(("at", i))
                                                .style(&caption_style())
                                                .show(ui);
                                        });
                                    Text::new(
                                        "Longer body that should wrap inside the Fill \
                                         column without breaking words inside any single \
                                         token.",
                                    )
                                    .id_salt(("msg", i))
                                    .style(&body_style())
                                    .text_wrap(TextWrap::Wrap)
                                    .size((Sizing::FILL, Sizing::HUG))
                                    .show(ui);
                                });
                        });
                }
            });
    });
}

fn properties_card(state: &mut FrameFixture, ui: &mut Ui, rows: usize) {
    card(ui, "props", "PROPERTIES", Sizing::HUG, |ui| {
        Grid::new()
            .id_salt("props-grid")
            .cols([Track::hug().min(92.0), Track::fill(), Track::fixed(60.0)])
            .rows(state.grid_rows.as_slice())
            .gap_xy(2.0, 8.0)
            .size((Sizing::FILL, Sizing::HUG))
            .show(ui, |ui| {
                const LABELS: [&str; 8] = [
                    "Name",
                    "Description",
                    "Author",
                    "License",
                    "Created",
                    "Modified",
                    "Tags",
                    "Notes",
                ];
                const VALUES: [&str; 4] = [
                    "the quick brown fox jumps over the lazy dog",
                    "Lorem ipsum dolor sit amet consectetur adipiscing elit sed do eiusmod tempor",
                    "Jane Doe and a long author name to force wrapping",
                    "MIT-or-Apache-2.0",
                ];
                for row in 0..rows {
                    let r = row as u16;
                    // Zebra band: a full-width cell under the row's three
                    // cells. Grid children may share a cell, so this both
                    // reads as a table and covers `grid_span`.
                    if row % 2 == 0 {
                        Frame::new()
                            .id_salt(("pband", row))
                            .size((Sizing::FILL, Sizing::FILL))
                            .background(Background {
                                fill: Color::hex(0x1f232d).into(),
                                corners: Corners::all(4.0),
                                ..Default::default()
                            })
                            .grid_cell((r, 0))
                            .grid_span((1, 3))
                            .show(ui);
                    }
                    Text::new(LABELS[row % LABELS.len()])
                        .id_salt(("plbl", row))
                        .style(&caption_style())
                        .margin(4.0)
                        .grid_cell((r, 0))
                        .show(ui);
                    Text::new(VALUES[row % VALUES.len()])
                        .id_salt(("pval", row))
                        .style(&body_style())
                        .text_wrap(TextWrap::Wrap)
                        .margin(4.0)
                        .grid_cell((r, 1))
                        .show(ui);
                    Button::new()
                        .id_salt(("pact", row))
                        .label("Edit")
                        .grid_cell((r, 2))
                        .show(ui);
                }
            });
    });
}

/// Tag chips beside a badge column — the two `WrapStack` orientations
/// side by side, each wrapping against a different bounded axis.
fn tags_card(ui: &mut Ui, tags: usize, badges: usize) {
    card(ui, "tags", "LABELS", Sizing::HUG, |ui| {
        Panel::hstack()
            .id_salt("tag-row")
            .gap(10.0)
            .size((Sizing::FILL, Sizing::HUG))
            .show(ui, |ui| {
                Panel::wrap_hstack()
                    .id_salt("tag-wrap")
                    .gap(4.0)
                    .size((Sizing::FILL, Sizing::HUG))
                    .show(ui, |ui| {
                        for i in 0..tags {
                            let label = ui.fmt(format_args!("#tag{i}"));
                            Button::new().id_salt(("tag", i)).label(label).show(ui);
                        }
                    });
                Separator::vertical().id_salt("tag-vsep").show(ui);
                // Wraps against its bounded *height*, so it fills column by
                // column. Each badge is a fixed-width chip: without one the
                // columns pack to their own text width and adjacent labels
                // read as a single run.
                Panel::wrap_vstack()
                    .id_salt("badge-wrap")
                    .gap(6.0)
                    .padding(6.0)
                    .size((Sizing::HUG, Sizing::fixed(96.0)))
                    .background(well_bg())
                    .show(ui, |ui| {
                        for i in 0..badges {
                            Text::new(ui.fmt(format_args!("badge {i}")))
                                .id_salt(("badge", i))
                                .style(&caption_style())
                                .size((Sizing::fixed(64.0), Sizing::HUG))
                                .show(ui);
                        }
                    });
            });
    });
}

fn notes_card(state: &mut FrameFixture, ui: &mut Ui) {
    card(ui, "notes", "NOTES", Sizing::HUG, |ui| {
        TextEdit::new(&mut state.notes)
            .id_salt("notes-edit")
            .size((Sizing::FILL, Sizing::fixed(56.0)))
            .show(ui);
    });
}

fn status_bar(state: &mut FrameFixture, ui: &mut Ui) {
    let bar = Panel::zstack()
        .id_salt("status")
        .size((Sizing::FILL, Sizing::fixed(34.0)))
        .show(ui, |ui| {
            Frame::new()
                .id_salt("footer-bg")
                .size((Sizing::FILL, Sizing::FILL))
                .background(Background {
                    fill: Brush::Linear(LinearGradient::two_stop(
                        0.0,
                        ColorU8::hex(0x1a1a2e),
                        ColorU8::hex(0x2a2a3e),
                    )),
                    corners: Corners::all(6.0),
                    ..Default::default()
                })
                .show(ui);
            Panel::hstack()
                .id_salt("footer-row")
                .padding(8.0)
                .gap(8.0)
                .child_align(Align::CENTER)
                .size((Sizing::FILL, Sizing::FILL))
                .show(ui, |ui| {
                    // Footer "live counter": the partial-damage arm mutates
                    // `state.tick` each iter. Fixed width pins layout so the
                    // changing digits can't shift siblings — damage collapses
                    // to this single Text node's arranged rect.
                    Text::new(ui.fmt(format_args!("Frame {:08}", state.tick)))
                        .id_salt("footer-status")
                        .style(&TextStyle::default().with_font_size(12.0))
                        .size((Sizing::fixed(120.0), Sizing::HUG))
                        .show(ui);
                    Frame::new()
                        .id_salt("footer-spacer")
                        .size((Sizing::FILL, Sizing::fixed(1.0)))
                        .show(ui);
                    Text::new("v1.2.3 · many nodes")
                        .id_salt("footer-meta")
                        .style(&caption_style())
                        .show(ui);
                });
        })
        .response
        .snapshot();

    // Toast on the Popup layer, parked above the *right* end of the status
    // bar — anchoring to the bar's full rect would drop it over the sidebar.
    // Reading the bar's rect (last frame's, hence the frame-0 fallback)
    // keeps it placed at any viewport size: `frame_visual` is 1280x800, the
    // bench target is far taller.
    let bar_rect = bar.rect.unwrap_or(Rect::new(12.0, 12.0, 240.0, 34.0));
    const TOAST_W: f32 = 220.0;
    let anchor = Rect::new(
        bar_rect.min.x + (bar_rect.size.w - TOAST_W).max(0.0),
        bar_rect.min.y,
        TOAST_W.min(bar_rect.size.w),
        bar_rect.size.h,
    );
    Popup::above(anchor)
        .background(Background {
            fill: CARD_BG.into(),
            stroke: Stroke::solid(BORDER, 1.0),
            corners: Corners::all(6.0),
            shadow: Shadow::drop(
                Color::rgba(0.0, 0.0, 0.0, 0.55),
                glam::Vec2::new(0.0, 3.0),
                10.0,
            ),
        })
        .show(ui, |ui, _handle| {
            Text::new("Capture written to disk")
                .id_salt("popup-label")
                .style(&caption_style())
                .show(ui);
        });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::display::Display;
    use crate::ui::frame_report::FramePaint;
    use crate::ui::harness::UiHarness;

    /// The `frame/partial_*` arms model an interactive steady state: one
    /// counter changes, everything else holds, so damage collapses to the
    /// footer Text's arranged rect. Pinned here — not only inside
    /// `ui::bench::assert_partial_invariant` — so a fixture edit that lets
    /// the counter reflow its siblings (→ `Full`) or hides the change from
    /// the tree entirely (→ `Skip`) fails `cargo test` instead of quietly
    /// retargeting the bench.
    ///
    /// Swept across viewport sizes because the `Skip` failure mode is a
    /// *layout* bug, not a damage bug: before the card column got its page
    /// scroll it overflowed on a normal-sized window and painted over the
    /// status bar, and an occluded counter damages nothing. The smallest
    /// size here is the one that regressed; the largest is the bench's own
    /// `CACHED_SIZE` / `BENCH_SCALE` pair.
    #[test]
    fn footer_counter_alone_yields_partial_damage() {
        for (px, scale) in [
            (glam::UVec2::new(1280, 800), 6usize),
            (glam::UVec2::new(2560, 1600), 6),
            (glam::UVec2::new(3840, 4800), 32),
        ] {
            let mut h = UiHarness::with_text(px);
            let mut state = FrameFixture::default();
            let display = Display::from_physical(px, 2.0);
            let mut paint = FramePaint::Full;
            // Two frames settle the caches and the popup's anchor (it reads
            // last frame's status-bar rect); the rest are steady state.
            for i in 0..5u64 {
                state.tick = state.tick.wrapping_add(1);
                paint = h
                    .frame_at(display, Duration::from_millis(i * 16), |ui| {
                        build_ui(&mut state, scale, ui)
                    })
                    .paint();
            }
            assert_eq!(
                paint,
                FramePaint::Partial,
                "tick-only change must damage just the footer counter at {px:?} @2x, scale {scale}",
            );
        }
    }
}
