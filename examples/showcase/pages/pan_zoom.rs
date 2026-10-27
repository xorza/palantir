//! `Scroll::both().zoomable()`: bare wheel pans, `Ctrl/Cmd + wheel` zooms about the cursor, pinch zooms.
//! A radio picks the content: a 24×24 button grid (~580 nodes) or a heavier mixed document (~2000 nodes) for
//! benchmarking; auto-drive pans and zooms every frame through `Scroll::pan_by`/`zoom_by`, the same clamp a wheel takes.

use crate::support;
use palantir::SlotDefaults;
use palantir::glam::Vec2;
use palantir::widget::{LineCap, LineJoin, Shape};
use palantir::{
    AnimationSpec, Block, Brush, Button, ButtonTheme, Checkbox, Chrome, Configure, Corners, Grid,
    LinearGradient, Panel, RadioButton, RgbaF32, Scroll, Sizing, Spacing, StatefulLook, Stroke,
    Text, TextStyle, TextStyleOverrides, TextWrap, Track, Ui, WidgetId, WidgetLook, fmt,
};
use std::array;

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
    /// Auto-drive frame counter, the oscillator's phase; reset when the drive stops.
    tick: u32,
    last_click: Option<CellPos>,
}

/// A button's place in a cell grid.
#[derive(Clone, Copy, Debug)]
struct CellPos {
    row: u32,
    col: u32,
}

impl CellPos {
    fn color(self) -> RgbaF32 {
        let tr = self.row as f32 / 24.0;
        let tc = self.col as f32 / 24.0;
        RgbaF32::srgb(
            0.30 + 0.55 * tc,
            0.55 - 0.25 * (tr - 0.5).abs(),
            0.85 - 0.55 * tr,
        )
    }
}

pub(crate) fn build(ui: &mut Ui) {
    let state_id = WidgetId::from_hash("showcase::pan_zoom::state");
    ui.with_state::<State, _>(state_id, page);
}

fn page(ui: &mut Ui, s: &mut State) {
    let mut auto_pan = Vec2::ZERO;
    let mut auto_zoom = 1.0;
    if s.auto {
        let t = s.tick as f32 * 0.05;
        auto_pan = Vec2::new(t.cos() * 5.0, (t * 0.7).cos() * 5.0);
        auto_zoom = 1.0 + t.cos() * 0.02;
        s.tick = s.tick.wrapping_add(1);
        ui.request_repaint();
    } else {
        s.tick = 0;
    }

    support::row(ui, |ui| {
        for (value, label) in [
            (Content::Grid, "button grid"),
            (Content::Document, "heavy document"),
        ] {
            RadioButton::new(&mut s.content, value)
                .id_salt(label)
                .label(label)
                .show(ui);
        }
        Checkbox::new(&mut s.auto)
            .label("auto-drive input")
            .show(ui);
        let click = match s.last_click {
            Some(CellPos { row, col }) => fmt!(ui, "row {row}, column {col}"),
            None => ui.intern("— click a cell to check hit-testing through the zoom"),
        };
        support::readout(ui, "last click", click);
    });

    let mut clicked = None;
    Scroll::both()
        .zoomable()
        .pan_by(auto_pan)
        .zoom_by(auto_zoom)
        .size((Sizing::FILL, Sizing::FILL))
        .show(ui, |ui| match s.content {
            Content::Grid => CellGrid {
                salt: "pz",
                rows: 24,
                cols: 24,
            }
            .show(ui, &mut clicked),
            Content::Document => document(ui, &mut clicked),
        });
    if clicked.is_some() {
        s.last_click = clicked;
    }
}

fn document(ui: &mut Ui, clicked: &mut Option<CellPos>) {
    Panel::vstack().gap(16.0).padding(8.0).show(ui, |ui| {
        header_band(ui);
        property_grid(ui);
        gradient_strip(ui);
        CellGrid {
            salt: "cells-a",
            rows: 24,
            cols: 24,
        }
        .show(ui, clicked);
        chat_messages(ui, 20);
        canvas_polylines(ui);
        CellGrid {
            salt: "cells-b",
            rows: 12,
            cols: 32,
        }
        .show(ui, clicked);
    });
}

fn header_band(ui: &mut Ui) {
    Panel::hstack()
        .gap(6.0)
        .size((Sizing::FILL, Sizing::HUG))
        .show(ui, |ui| {
            Text::new("Complex document")
                .style(&TextStyle::default().with_font_size(18.0))
                .show(ui);
            Block::new()
                .size((Sizing::FILL, Sizing::fixed(1.0)))
                .show(ui);
            for i in 0..6 {
                Button::new()
                    .id_salt(i)
                    .label(fmt!(ui, "Action {i}"))
                    .show(ui);
            }
        });
}

fn property_grid(ui: &mut Ui) {
    const ROWS: usize = 12;
    Grid::new()
        .cols([Track::HUG.with_min(96.0), Track::FILL, Track::fixed(72.0)])
        .rows([Track::HUG; ROWS])
        .line_gap(6.0)
        .gap(6.0)
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
        .gap(6.0)
        .size((Sizing::FILL, Sizing::HUG))
        .show(ui, |ui| {
            for i in 0..10 {
                let t = i as f32 / 10.0;
                let a = RgbaF32::srgb(0.2 + 0.6 * t, 0.4, 0.9 - 0.6 * t);
                let b = RgbaF32::srgb(0.95 - 0.5 * t, 0.7 * t, 0.3 + 0.5 * t);
                Block::new()
                    .id_salt(i)
                    .size((Sizing::fixed(72.0), Sizing::fixed(56.0)))
                    .chrome(Chrome {
                        fill: Brush::Linear(LinearGradient::two_stop(0.0, a, b)),
                        corners: Corners::all(6.0),
                        border: Stroke::new(RgbaF32::hex(0x202020), 1.0),
                        ..Default::default()
                    })
                    .show(ui);
            }
        });
}

/// A `rows` by `cols` grid of clickable cells under `salt`.
#[derive(Debug)]
struct CellGrid {
    salt: &'static str,
    rows: u32,
    cols: u32,
}

impl CellGrid {
    /// Records the grid, writing the clicked cell to `clicked`.
    fn show(self, ui: &mut Ui, clicked: &mut Option<CellPos>) {
        // One theme for the whole grid, repointed per cell: building a `ButtonTheme` per cell would dominate per-cell work.
        let mut style = cell_theme();
        Panel::vstack().id_salt(self.salt).gap(4.0).show(ui, |ui| {
            for row in 0..self.rows {
                Panel::hstack().id_salt(row).gap(4.0).show(ui, |ui| {
                    for col in 0..self.cols {
                        let pos = CellPos { row, col };
                        recolor_cell(&mut style, pos.color());
                        if cell(ui, pos, &style) {
                            *clicked = Some(pos);
                        }
                    }
                });
            }
        });
    }
}

fn cell(ui: &mut Ui, pos: CellPos, style: &ButtonTheme) -> bool {
    // Formatted into the record arena, not a `String`: this runs for every cell every frame.
    let label = fmt!(ui, "{},{}", pos.row, pos.col);
    Button::new()
        .id_salt(pos.col)
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
        .gap(8.0)
        .size((Sizing::FILL, Sizing::HUG))
        .show(ui, |ui| {
            for i in 0..count {
                Panel::hstack()
                    .id_salt(i)
                    .gap(8.0)
                    .size((Sizing::FILL, Sizing::HUG))
                    .show(ui, |ui| {
                        Block::new()
                            .size((Sizing::fixed(40.0), Sizing::fixed(40.0)))
                            .chrome(Chrome::rounded(
                                CellPos { row: i, col: i / 3 }.color(),
                                Corners::all(20.0),
                            ))
                            .show(ui);
                        Panel::vstack()
                            .gap(2.0)
                            .size((Sizing::FILL, Sizing::HUG))
                            .show(ui, |ui| {
                                Text::new(fmt!(ui, "user_{i}"))
                                    .style(&TextStyle::default().with_font_size(12.0))
                                    .show(ui);
                                Text::new(
                                    "Lorem ipsum dolor sit amet consectetur adipiscing elit sed \
                                     do eiusmod tempor incididunt ut labore et dolore magna aliqua.",
                                )
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
        .size((Sizing::FILL, Sizing::fixed(120.0)))
        .chrome(
            Chrome::rounded(support::WELL, Corners::all(4.0))
                .with_border(Stroke::new(support::BORDER, 1.0)),
        )
        .show(ui, |ui| {
            Block::new().size((Sizing::FILL, Sizing::FILL)).show(ui);
            for line in 0..6 {
                // Fixed count, so the points sit on the stack instead of a `Vec` per line per frame.
                let pts: [Vec2; 32] = array::from_fn(|i| {
                    let x = i as f32 * 24.0 + 8.0;
                    let phase = line as f32 * 0.6 + i as f32 * 0.25;
                    let y = 60.0 + phase.sin() * (16.0 + line as f32 * 3.0);
                    Vec2::new(x, y)
                });
                let c = RgbaF32::srgb(
                    0.4 + line as f32 * 0.1,
                    0.85 - line as f32 * 0.08,
                    0.4 + (1 + line) as f32 * 0.07,
                );
                ui.add_shape(
                    Shape::polyline(&pts, Stroke::new(c, 1.5))
                        .cap(LineCap::Round)
                        .join(LineJoin::Round),
                );
            }
        });
}

/// What a cell's `ButtonTheme` shares; [`recolor_cell`] sets the four backgrounds, the only per-cell difference.
fn cell_theme() -> ButtonTheme {
    let label = TextStyleOverrides::NONE
        .with_font_size(11.0)
        .with_color(RgbaF32::hex(0x14161a));
    let look = || WidgetLook {
        chrome: Chrome::NONE,
        text: label,
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
            animation: Some(AnimationSpec::FAST),
        },
    }
}

/// Points `theme`'s four states at one cell's color.
fn recolor_cell(theme: &mut ButtonTheme, base: RgbaF32) {
    let chrome = |fill: RgbaF32| Chrome::rounded(fill, Corners::all(3.0));
    theme.looks.normal.chrome = chrome(base);
    theme.looks.hovered.chrome = chrome(brighten(base, 0.15));
    theme.looks.active.chrome =
        chrome(brighten(base, 0.3)).with_border(Stroke::new(RgbaF32::WHITE, 1.0));
    theme.looks.disabled.chrome = chrome(base);
}

const fn brighten(c: RgbaF32, t: f32) -> RgbaF32 {
    support::mix(c, RgbaF32::WHITE.with_alpha(c.a), t)
}
