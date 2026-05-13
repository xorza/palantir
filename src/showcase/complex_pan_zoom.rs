//! Heavier sibling of `pan_zoom`. Same outer `Scroll::both().with_zoom()`
//! wrapper, but the inner viewport is a long vertical document built
//! from a mix of drivers (vstack / hstack / grid / canvas), wrapping
//! text, gradient swatches, polyline shapes, and two button grids —
//! ~2000 nodes total vs `pan_zoom`'s ~580. The auto-demo tab and
//! `benches/scrollzoom.rs` import this file so the bench's workload
//! matches what you see in the showcase.
//!
//! Self-contained: only `palantir::` items, no `crate::` references,
//! so `#[path]` includes from `benches/` / `examples/` work.

use glam::Vec2;
use palantir::{
    AnimSpec, Background, Brush, Button, ButtonTheme, Color, Configure, Corners, Frame, Grid,
    LineCap, LineJoin, LinearGradient, Panel, PolylineColors, Scroll, Shape, Sizing, Spacing,
    Stroke, Text, TextStyle, Track, Ui, WidgetLook,
};
use std::rc::Rc;

pub fn build(ui: &mut Ui) {
    Panel::vstack()
        .auto_id()
        .gap(8.0)
        .size((Sizing::FILL, Sizing::FILL))
        .show(ui, |ui| {
            Text::new(
                "Complex pan + zoom — heavier scroll content for benchmarking. \
                 Wheel pans, Ctrl/Cmd + wheel zooms about the cursor.",
            )
            .auto_id()
            .wrapping()
            .style(TextStyle::default().with_font_size(13.0))
            .show(ui);

            Scroll::both()
                .auto_id()
                .with_zoom()
                .size((Sizing::FILL, Sizing::FILL))
                .show(ui, |ui| {
                    Panel::vstack()
                        .id_salt("doc")
                        .gap(16.0)
                        .padding(8.0)
                        .show(ui, |ui| {
                            header_band(ui);
                            property_grid(ui);
                            gradient_strip(ui);
                            cell_grid(ui, "cells-a", 24, 24);
                            chat_messages(ui, 20);
                            canvas_polylines(ui);
                            cell_grid(ui, "cells-b", 12, 32);
                        });
                });
        });
}

fn header_band(ui: &mut Ui) {
    Panel::hstack()
        .id_salt("hdr")
        .gap(6.0)
        .size((Sizing::FILL, Sizing::Hug))
        .show(ui, |ui| {
            Text::new("Complex showcase")
                .auto_id()
                .style(TextStyle::default().with_font_size(18.0))
                .show(ui);
            Frame::new()
                .id_salt("hdr-spacer")
                .size((Sizing::FILL, Sizing::Fixed(1.0)))
                .show(ui);
            for i in 0..6 {
                Button::new()
                    .id_salt(("hdr-btn", i))
                    .label(format!("Action {i}"))
                    .show(ui);
            }
        });
}

fn property_grid(ui: &mut Ui) {
    const ROWS: usize = 12;
    let rows: Vec<Track> = (0..ROWS).map(|_| Track::hug()).collect();
    Grid::new()
        .id_salt("props")
        .cols(Rc::from([
            Track::hug().min(96.0),
            Track::fill(),
            Track::fixed(72.0),
        ]))
        .rows(Rc::<[Track]>::from(rows))
        .gap(6.0)
        .padding(4.0)
        .size((Sizing::FILL, Sizing::Hug))
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
                "pan, zoom, scroll, layout, benchmark, golden, snapshot",
                "Heavier scroll content for the scrollzoom benchmark.",
            ];
            for row in 0..ROWS {
                let r = row as u16;
                Text::new(labels[row % labels.len()])
                    .id_salt(("plbl", row))
                    .style(TextStyle::default().with_font_size(14.0))
                    .grid_cell((r, 0))
                    .show(ui);
                Text::new(values[row % values.len()])
                    .id_salt(("pval", row))
                    .style(TextStyle::default().with_font_size(14.0))
                    .wrapping()
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
        .size((Sizing::FILL, Sizing::Hug))
        .show(ui, |ui| {
            for i in 0..10 {
                let t = i as f32 / 10.0;
                let a = Color::rgb(0.2 + 0.6 * t, 0.4, 0.9 - 0.6 * t);
                let b = Color::rgb(0.95 - 0.5 * t, 0.7 * t, 0.3 + 0.5 * t);
                Frame::new()
                    .id_salt(("grad", i))
                    .size((Sizing::Fixed(72.0), Sizing::Fixed(56.0)))
                    .background(Background {
                        fill: Brush::Linear(LinearGradient::two_stop(0.0, a, b)),
                        radius: Corners::all(6.0),
                        stroke: Stroke::solid(Color::hex(0x202020), 1.0),
                        shadow: None,
                    })
                    .show(ui);
            }
        });
}

fn cell_grid(ui: &mut Ui, salt: &'static str, rows: u32, cols: u32) {
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
                            cell(ui, salt, r, c);
                        }
                    });
            }
        });
}

fn cell(ui: &mut Ui, salt: &'static str, r: u32, c: u32) {
    Button::new()
        .id_salt((salt, "cell", r, c))
        .label(format!("{r},{c}"))
        .size((Sizing::Fixed(56.0), Sizing::Fixed(40.0)))
        .padding((6.0, 4.0))
        .style(cell_theme(r, c))
        .show(ui);
}

fn chat_messages(ui: &mut Ui, count: u32) {
    Panel::vstack()
        .id_salt("chat")
        .gap(8.0)
        .size((Sizing::FILL, Sizing::Hug))
        .show(ui, |ui| {
            for i in 0..count {
                Panel::hstack()
                    .id_salt(("chat-row", i))
                    .gap(8.0)
                    .size((Sizing::FILL, Sizing::Hug))
                    .show(ui, |ui| {
                        Frame::new()
                            .id_salt(("avatar", i))
                            .size((Sizing::Fixed(40.0), Sizing::Fixed(40.0)))
                            .background(Background {
                                fill: cell_color(i, i / 3).into(),
                                radius: Corners::all(20.0),
                                ..Default::default()
                            })
                            .show(ui);
                        Panel::vstack()
                            .id_salt(("chat-text", i))
                            .gap(2.0)
                            .size((Sizing::FILL, Sizing::Hug))
                            .show(ui, |ui| {
                                Text::new(format!("user_{i}"))
                                    .id_salt(("from", i))
                                    .style(TextStyle::default().with_font_size(12.0))
                                    .show(ui);
                                Text::new(
                                    "Lorem ipsum dolor sit amet consectetur adipiscing elit sed \
                                     do eiusmod tempor incididunt ut labore et dolore magna aliqua.",
                                )
                                .id_salt(("msg", i))
                                .style(TextStyle::default().with_font_size(13.0))
                                .wrapping()
                                .size((Sizing::FILL, Sizing::Hug))
                                .show(ui);
                            });
                    });
            }
        });
}

fn canvas_polylines(ui: &mut Ui) {
    Panel::canvas()
        .id_salt("polylines")
        .size((Sizing::FILL, Sizing::Fixed(120.0)))
        .background(Background {
            fill: Color::hex(0x1a1a1a).into(),
            radius: Corners::all(4.0),
            stroke: Stroke::solid(Color::hex(0x303030), 1.0),
            shadow: None,
        })
        .show(ui, |ui| {
            Frame::new()
                .id_salt("poly-host")
                .size((Sizing::FILL, Sizing::FILL))
                .show(ui);
            for line in 0..6 {
                let mut pts: Vec<Vec2> = (0..32)
                    .map(|i| {
                        let x = i as f32 * 24.0 + 8.0;
                        let phase = line as f32 * 0.6 + i as f32 * 0.25;
                        let y = 60.0 + phase.sin() * (16.0 + line as f32 * 3.0);
                        Vec2::new(x, y)
                    })
                    .collect();
                pts.dedup_by(|a, b| (a.x - b.x).abs() < 0.01 && (a.y - b.y).abs() < 0.01);
                let c = Color::rgb(
                    0.4 + line as f32 * 0.1,
                    0.85 - line as f32 * 0.08,
                    0.4 + (1 + line) as f32 * 0.07,
                );
                ui.add_shape(Shape::Polyline {
                    points: &pts,
                    colors: PolylineColors::Single(c),
                    width: 1.5,
                    cap: LineCap::Round,
                    join: LineJoin::Round,
                });
            }
        });
}

fn cell_theme(r: u32, c: u32) -> ButtonTheme {
    let base = cell_color(r, c);
    let bg = |fill: Color| -> Background {
        Background {
            fill: fill.into(),
            radius: Corners::all(3.0),
            ..Default::default()
        }
    };
    let pressed_bg = Background {
        fill: brighten(base, 0.3).into(),
        stroke: Stroke::solid(Color::hex(0xffffff), 1.0),
        radius: Corners::all(3.0),
        shadow: None,
    };
    let label_text = TextStyle::default()
        .with_font_size(11.0)
        .with_color(Color::hex(0x1a1a1a));
    ButtonTheme {
        normal: WidgetLook {
            background: Some(bg(base)),
            text: Some(label_text),
        },
        hovered: WidgetLook {
            background: Some(bg(brighten(base, 0.15))),
            text: Some(label_text),
        },
        pressed: WidgetLook {
            background: Some(pressed_bg),
            text: Some(label_text),
        },
        disabled: WidgetLook {
            background: Some(bg(base)),
            text: Some(label_text),
        },
        padding: Spacing::xy(6.0, 4.0),
        margin: Spacing::ZERO,
        anim: Some(AnimSpec::FAST),
    }
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
