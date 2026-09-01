//! `Scroll::both().with_zoom()` — bare wheel pans, `Ctrl/Cmd + wheel`
//! zooms about the cursor, pinch zooms unconditionally. Pin the cursor
//! to a cell and scroll-zoom: the cell stays under the cursor.
//!
//! The radio picks the viewport content: a dense 24×24 button grid
//! (~580 nodes — cells are buttons so hover / press / click input works
//! through the transform), or a heavier mixed document (~2000 nodes:
//! grids, wrapping text, gradient swatches, polylines, chat rows) for
//! benchmarking. The auto-drive checkbox feeds synthetic scroll + zoom
//! input every frame from a bounded cosine oscillator, with the pointer
//! seeded over the viewport once so `scroll_target` latches without
//! clobbering the real cursor on later frames.

use crate::support;
use crate::support::note_style;
use palantir::SlotDefaults;
use palantir::{
    AnimSpec, Background, Brush, Button, ButtonTheme, Checkbox, Color, Configure, Corners, Frame,
    Grid, InputEvent, LineCap, LineJoin, LinearGradient, Panel, PolylineColors, RadioButton,
    Scroll, Shape, Sizing, Spacing, StatefulLook, Stroke, Text, TextStyle, TextWrap, Track, Ui,
    Vec2, WidgetId, WidgetLook, fmt,
};

#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
enum Content {
    #[default]
    Grid,
    Document,
}

#[derive(Default, Debug)]
struct State {
    content: Content,
    auto: bool,
    /// Auto-drive frame counter; 0 means "seed the pointer this frame".
    tick: u32,
    last_click: Option<(u32, u32)>,
}

pub(crate) fn build(ui: &mut Ui) {
    let state_id = WidgetId::from_hash("showcase::pan_zoom::state");
    ui.with_state::<State, _>(state_id, page);
}

fn page(ui: &mut Ui, s: &mut State) {
    if s.auto {
        // Seed the pointer over the scroll viewport on the first frame
        // only — enough to latch scroll_target. Re-injecting every frame
        // would clobber the real cursor and break clicks on the rail.
        if s.tick == 0 {
            let size = ui.display().logical_size();
            let centre = Vec2::new(size.w * 0.6, size.h * 0.6);
            ui.on_input(InputEvent::PointerMoved(centre));
        }
        let t = s.tick as f32 * 0.05;
        ui.on_input(InputEvent::ScrollPixels(Vec2::new(
            t.cos() * 5.0,
            (t * 0.7).cos() * 5.0,
        )));
        ui.on_input(InputEvent::Zoom(1.0 + t.cos() * 0.02));
        s.tick = s.tick.wrapping_add(1);
        ui.request_repaint();
    } else {
        s.tick = 0;
    }

    Panel::hstack()
        .id_salt("pz-controls")
        .gap(16.0)
        .size((Sizing::FILL, Sizing::HUG))
        .show(ui, |ui| {
            for (value, label) in [
                (Content::Grid, "button grid"),
                (Content::Document, "heavy document"),
            ] {
                RadioButton::new(&mut s.content, value)
                    .id_salt(("pz-content", label))
                    .label(label)
                    .show(ui);
            }
            Checkbox::new(&mut s.auto)
                .id_salt("pz-auto")
                .label("auto-drive input")
                .show(ui);
            let click = match s.last_click {
                Some((r, c)) => fmt!(ui, "last click: r{r} c{c}"),
                None => fmt!(
                    ui,
                    "click a cell to confirm hit-testing through the transform"
                ),
            };
            Text::new(click)
                .id_salt("pz-click")
                .style(&note_style())
                .show(ui);
        });

    let mut clicked = None;
    Scroll::both()
        .with_zoom()
        .size((Sizing::FILL, Sizing::FILL))
        .show(ui, |ui| match s.content {
            Content::Grid => cell_grid(ui, "pz", 24, 24, &mut clicked),
            Content::Document => document(ui, &mut clicked),
        });
    if clicked.is_some() {
        s.last_click = clicked;
    }
}

/// The heavy mixed document — a long vertical run of grids, wrapping
/// text, gradient swatches, polylines, and button grids.
fn document(ui: &mut Ui, clicked: &mut Option<(u32, u32)>) {
    Panel::vstack()
        .id_salt("doc")
        .gap(16.0)
        .padding(8.0)
        .show(ui, |ui| {
            header_band(ui);
            property_grid(ui);
            gradient_strip(ui);
            cell_grid(ui, "cells-a", 24, 24, clicked);
            chat_messages(ui, 20);
            canvas_polylines(ui);
            cell_grid(ui, "cells-b", 12, 32, clicked);
        });
}

fn header_band(ui: &mut Ui) {
    Panel::hstack()
        .id_salt("hdr")
        .gap(6.0)
        .size((Sizing::FILL, Sizing::HUG))
        .show(ui, |ui| {
            Text::new("Complex document")
                .style(&TextStyle::default().with_font_size(18.0))
                .show(ui);
            Frame::new()
                .id_salt("hdr-spacer")
                .size((Sizing::FILL, Sizing::fixed(1.0)))
                .show(ui);
            for i in 0..6 {
                Button::new()
                    .id_salt(("hdr-btn", i))
                    .label(fmt!(ui, "Action {i}"))
                    .show(ui);
            }
        });
}

fn property_grid(ui: &mut Ui) {
    const ROWS: usize = 12;
    Grid::new()
        .id_salt("props")
        .cols([Track::HUG.min(96.0), Track::FILL, Track::fixed(72.0)])
        .rows([Track::HUG; ROWS])
        .line_gap(6.0).gap(6.0)
        .padding(4.0)
        .size((Sizing::FILL, Sizing::HUG))
        .show(ui, |ui| {
            let labels = [
                "Name",
                "Description",
                "Author",
                "License",
                "Created",
                "Modified",
                "Tags",
                "Notes",
                "Status",
                "Version",
                "Dependencies",
                "Notes",
            ];
            let values = [
                "the quick brown fox jumps over the lazy dog",
                "Lorem ipsum dolor sit amet consectetur adipiscing elit sed do eiusmod tempor incididunt ut labore et dolore magna",
                "Jane Doe and a longer author name that wraps across two lines in narrow viewports",
                "MIT-or-Apache-2.0",
                "2025-04-12",
                "2026-05-12",
                "pan, zoom, scroll, layout, golden, snapshot",
                "Heavier scroll content for the pan & zoom page.",
            ];
            for row in 0..ROWS {
                let r = row as u16;
                Text::new(labels[row % labels.len()])
                    .id_salt(("plbl", row))
                    .style(&TextStyle::default().with_font_size(14.0))
                    .grid_cell((r, 0))
                    .show(ui);
                Text::new(values[row % values.len()])
                    .id_salt(("pval", row))
                    .style(&TextStyle::default().with_font_size(14.0))
                    .text_wrap(TextWrap::WrapWithOverflow)
                    .grid_cell((r, 1))
                    .show(ui);
                Button::new()
                    .id_salt(("pact", row))
                    .label("Edit")
                    .grid_cell((r, 2))
                    .show(ui);
            }
        });
}

fn gradient_strip(ui: &mut Ui) {
    Panel::hstack()
        .id_salt("grad-row")
        .gap(6.0)
        .size((Sizing::FILL, Sizing::HUG))
        .show(ui, |ui| {
            for i in 0..10 {
                let t = i as f32 / 10.0;
                let a = Color::rgb(0.2 + 0.6 * t, 0.4, 0.9 - 0.6 * t);
                let b = Color::rgb(0.95 - 0.5 * t, 0.7 * t, 0.3 + 0.5 * t);
                Frame::new()
                    .id_salt(("grad", i))
                    .size((Sizing::fixed(72.0), Sizing::fixed(56.0)))
                    .background(Background {
                        fill: Brush::Linear(LinearGradient::two_stop(0.0, a, b)),
                        corners: Corners::all(6.0),
                        stroke: Stroke::solid(Color::hex(0x202020), 1.0),
                        ..Default::default()
                    })
                    .show(ui);
            }
        });
}

fn cell_grid(
    ui: &mut Ui,
    salt: &'static str,
    rows: u32,
    cols: u32,
    clicked: &mut Option<(u32, u32)>,
) {
    // One theme for the whole grid, repointed per cell: only the fill
    // varies, and a `ButtonTheme` is four `WidgetLook`s deep — building
    // one per cell was the largest piece of per-cell work on the page.
    let mut style = cell_theme();
    Panel::vstack()
        .id_salt((salt, "v"))
        .gap(4.0)
        .show(ui, |ui| {
            for r in 0..rows {
                Panel::hstack()
                    .id_salt((salt, "row", r))
                    .gap(4.0)
                    .show(ui, |ui| {
                        for c in 0..cols {
                            recolor_cell(&mut style, cell_color(r, c));
                            if cell(ui, salt, r, c, &style) {
                                *clicked = Some((r, c));
                            }
                        }
                    });
            }
        });
}

fn cell(ui: &mut Ui, salt: &'static str, r: u32, c: u32, style: &ButtonTheme) -> bool {
    // Formatted into the record arena rather than a `String`: this runs for
    // every cell of every grid every frame, and the page is the pan/zoom
    // benchmark workload.
    let label = fmt!(ui, "{r},{c}");
    Button::new()
        .id_salt((salt, "cell", r, c))
        .label(label)
        .size((Sizing::fixed(56.0), Sizing::fixed(40.0)))
        .padding((6.0, 4.0))
        .style(style)
        .show(ui)
        .left
        .clicked()
}

fn chat_messages(ui: &mut Ui, count: u32) {
    Panel::vstack()
        .id_salt("chat")
        .gap(8.0)
        .size((Sizing::FILL, Sizing::HUG))
        .show(ui, |ui| {
            for i in 0..count {
                Panel::hstack()
                    .id_salt(("chat-row", i))
                    .gap(8.0)
                    .size((Sizing::FILL, Sizing::HUG))
                    .show(ui, |ui| {
                        Frame::new()
                            .id_salt(("avatar", i))
                            .size((Sizing::fixed(40.0), Sizing::fixed(40.0)))
                            .background(Background::rounded(
                                cell_color(i, i / 3),
                                Corners::all(20.0),
                            ))
                            .show(ui);
                        Panel::vstack()
                            .id_salt(("chat-text", i))
                            .gap(2.0)
                            .size((Sizing::FILL, Sizing::HUG))
                            .show(ui, |ui| {
                                Text::new(fmt!(ui, "user_{i}"))
                                    .id_salt(("from", i))
                                    .style(&TextStyle::default().with_font_size(12.0))
                                    .show(ui);
                                Text::new(
                                    "Lorem ipsum dolor sit amet consectetur adipiscing elit sed \
                                     do eiusmod tempor incididunt ut labore et dolore magna aliqua.",
                                )
                                .id_salt(("msg", i))
                                .style(&TextStyle::default().with_font_size(13.0))
                                .text_wrap(TextWrap::WrapWithOverflow)
                                .size((Sizing::FILL, Sizing::HUG))
                                .show(ui);
                            });
                    });
            }
        });
}

fn canvas_polylines(ui: &mut Ui) {
    Panel::canvas()
        .id_salt("polylines")
        .size((Sizing::FILL, Sizing::fixed(120.0)))
        .background(
            Background::rounded(support::WELL, Corners::all(4.0))
                .with_stroke(Stroke::solid(support::BORDER, 1.0)),
        )
        .show(ui, |ui| {
            Frame::new()
                .id_salt("poly-host")
                .size((Sizing::FILL, Sizing::FILL))
                .show(ui);
            for line in 0..6 {
                // Fixed count, so the points sit on the stack — a `Vec` per
                // line per frame was six allocations for a shape whose size
                // is a literal. Adjacent points are 24 px apart in x, so
                // there is nothing for a coincident-point dedup to find.
                let pts: [Vec2; 32] = std::array::from_fn(|i| {
                    let x = i as f32 * 24.0 + 8.0;
                    let phase = line as f32 * 0.6 + i as f32 * 0.25;
                    let y = 60.0 + phase.sin() * (16.0 + line as f32 * 3.0);
                    Vec2::new(x, y)
                });
                let c = Color::rgb(
                    0.4 + line as f32 * 0.1,
                    0.85 - line as f32 * 0.08,
                    0.4 + (1 + line) as f32 * 0.07,
                );
                ui.add_shape(
                    Shape::polyline(&pts, PolylineColors::Single(c), 1.5)
                        .cap(LineCap::Round)
                        .join(LineJoin::Round),
                );
            }
        });
}

/// Everything a cell's `ButtonTheme` shares: the label ink, the padding,
/// and the anim that drives a smooth fill transition on hover and press.
/// The four backgrounds are left to [`recolor_cell`], which is all that
/// differs between cells.
fn cell_theme() -> ButtonTheme {
    let label = TextStyle::default()
        .with_font_size(11.0)
        .with_color(Color::hex(0x14161a));
    let look = || WidgetLook {
        background: Background::NONE,
        text: Some(label),
    };
    ButtonTheme {
        looks: StatefulLook {
            normal: look(),
            hovered: look(),
            active: look(),
            disabled: look(),
        },
        defaults: SlotDefaults {
            padding: Spacing::xy(6.0, 4.0),
            margin: Spacing::ZERO,
            anim: Some(AnimSpec::FAST),
        },
    }
}

/// Point `theme`'s four states at one cell's colour: normal is the base,
/// hovered is brightened, pressed is brightest with a focus stroke.
fn recolor_cell(theme: &mut ButtonTheme, base: Color) {
    let bg = |fill: Color| Background::rounded(fill, Corners::all(3.0));
    theme.looks.normal.background = bg(base);
    theme.looks.hovered.background = bg(brighten(base, 0.15));
    theme.looks.active.background =
        bg(brighten(base, 0.3)).with_stroke(Stroke::solid(Color::WHITE, 1.0));
    theme.looks.disabled.background = bg(base);
}

fn brighten(c: Color, t: f32) -> Color {
    Color::linear_rgba(
        c.r + (1.0 - c.r) * t,
        c.g + (1.0 - c.g) * t,
        c.b + (1.0 - c.b) * t,
        c.a,
    )
}

fn cell_color(r: u32, c: u32) -> Color {
    let tr = r as f32 / 24.0;
    let tc = c as f32 / 24.0;
    Color::rgb(
        0.30 + 0.55 * tc,
        0.55 - 0.25 * (tr - 0.5).abs(),
        0.85 - 0.55 * tr,
    )
}
