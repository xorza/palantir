//! Container drivers: the four stack panels (HStack / VStack / ZStack /
//! Canvas), flow-then-wrap WrapHStack / WrapVStack (`.gap` spaces
//! siblings within a line, `.line_gap` spaces lines, `.justify` applies
//! per line), and Grid with fixed / fill / hug / clamped tracks plus
//! cell spanning.

use crate::support;
use crate::support::{demo_cell, demo_cell_at, on_swatch_style, section, swatch_bg, tiles};
use palantir::{
    Align, Background, Configure, Corners, Frame, Grid, GridCell, Justify, Panel, RgbaF32, Sizing,
    Stroke, Text, TextStyle, Track, Ui,
};
use std::hash::Hash;

pub(crate) fn build(ui: &mut Ui) {
    section(ui, "stacks — one axis each, plus free positioning", |ui| {
        tiles(ui, |ui| {
            demo_cell(ui, "HStack — left to right", |ui| {
                Panel::hstack().gap(6.0).show(ui, |ui| {
                    for k in ["h-a", "h-b", "h-c"] {
                        sw(ui, k, 40.0, 40.0, support::A);
                    }
                });
            });
            demo_cell(ui, "VStack — top to bottom", |ui| {
                Panel::vstack().gap(6.0).show(ui, |ui| {
                    for k in ["v-a", "v-b", "v-c"] {
                        sw(ui, k, 60.0, 24.0, support::A);
                    }
                });
            });
            demo_cell(ui, "ZStack — stacked in record order", |ui| {
                Panel::zstack().child_align(Align::CENTER).show(ui, |ui| {
                    sw(ui, "z-back", 96.0, 96.0, support::A);
                    sw(ui, "z-front", 56.0, 56.0, support::B);
                });
            });
            demo_cell(ui, "Canvas — positioned children", |ui| {
                Panel::canvas()
                    .size((Sizing::FILL, Sizing::FILL))
                    .show(ui, |ui| {
                        positioned(ui, "p1", 8.0, 8.0, support::A);
                        positioned(ui, "p2", 64.0, 40.0, support::B);
                        positioned(ui, "p3", 30.0, 88.0, support::C);
                    });
            });
        });
    });

    section(
        ui,
        "wrapping flow — children flow along the main axis and wrap when the next \
         one wouldn't fit",
        |ui| {
            Panel::wrap_hstack()
                .id_salt("tags")
                .size((Sizing::FILL, Sizing::HUG))
                .gap(8.0)
                .line_gap(8.0)
                .show(ui, |ui| {
                    for (i, t) in TAGS.iter().enumerate() {
                        chip(ui, ("tag", i), t);
                    }
                });
        },
    );

    section(
        ui,
        "wrap axis — WrapVStack wraps to a new column; Justify applies per line, \
         not to the block",
        |ui| {
            tiles(ui, |ui| {
                demo_cell_at(ui, "WrapVStack — wraps into columns", 200.0, 200.0, |ui| {
                    Panel::wrap_vstack()
                        .id_salt("vwrap")
                        .size((Sizing::HUG, Sizing::FILL))
                        .gap(6.0)
                        .line_gap(12.0)
                        .show(ui, |ui| {
                            for i in 0..12 {
                                badge(ui, ("v", i), 70.0);
                            }
                        });
                });
                demo_cell_at(
                    ui,
                    "WrapHStack — Justify::Center per line",
                    200.0,
                    200.0,
                    |ui| {
                        Panel::wrap_hstack()
                            .id_salt("centered")
                            .size((Sizing::FILL, Sizing::HUG))
                            .gap(10.0)
                            .line_gap(10.0)
                            .justify(Justify::Center)
                            .show(ui, |ui| {
                                for i in 0..7 {
                                    badge(ui, ("c", i), 52.0);
                                }
                            });
                    },
                );
            });
        },
    );

    section(
        ui,
        "grid — two-axis tracks with spanning cells; resize the window to watch \
         Fill tracks re-divide",
        |ui| {
            tiles(ui, |ui| {
                // Classic three-column app shell: fixed sidebar | flexible
                // content | hugging right rail; the header spans all three.
                demo_cell_at(
                    ui,
                    "app shell — fixed | fill | hug, header spans",
                    280.0,
                    200.0,
                    |ui| {
                        Grid::new()
                            .id_salt("shell-grid")
                            .cols([Track::fixed(80.0), Track::FILL, Track::HUG])
                            .rows([Track::fixed(32.0), Track::FILL])
                            .line_gap(8.0)
                            .gap(8.0)
                            .size((Sizing::FILL, Sizing::FILL))
                            .show(ui, |ui| {
                                grid_tile(ui, "header", (0, 0), Some((1, 3)), None, support::B);
                                grid_tile(ui, "nav", (1, 0), None, None, support::C);
                                grid_tile(ui, "content", (1, 1), None, None, support::A);
                                grid_tile(
                                    ui,
                                    "rail",
                                    (1, 2),
                                    None,
                                    Some((Sizing::fixed(56.0), Sizing::FILL)),
                                    support::D,
                                );
                            });
                    },
                );
                // The left Fill is bounded [110, 160] so it grows with the
                // tile only within that range; the right Fill absorbs every
                // leftover pixel.
                demo_cell_at(
                    ui,
                    "clamped track — Fill min 110 max 160 | Fill",
                    280.0,
                    200.0,
                    |ui| {
                        Grid::new()
                            .id_salt("clamped")
                            .cols([Track::fill(1.0).min(110.0).max(160.0), Track::fill(2.0)])
                            .rows([Track::FILL])
                            .line_gap(8.0)
                            .gap(8.0)
                            .size((Sizing::FILL, Sizing::FILL))
                            .show(ui, |ui| {
                                grid_tile(ui, "sidebar", (0, 0), None, None, support::A);
                                grid_tile(ui, "main", (0, 1), None, None, support::D);
                            });
                    },
                );
            });
        },
    );
}

const TAGS: &[&str] = &[
    "rust",
    "wgpu",
    "layout",
    "intrinsic",
    "WrapHStack",
    "flexbox-ish",
    "no-grid",
    "tags",
    "demo",
    "hug",
    "fill",
    "fixed",
    "padding",
    "margin",
    "z-order",
    "sdf",
    "rounded",
    "stroke",
    "alpha",
    "linear",
];

fn sw(ui: &mut Ui, id: &'static str, w: f32, h: f32, c: RgbaF32) {
    support::swatch(ui, id, (Sizing::fixed(w), Sizing::fixed(h)), c);
}

fn positioned(ui: &mut Ui, id: &'static str, x: f32, y: f32, c: RgbaF32) {
    Frame::new()
        .id_salt(id)
        .position((x, y))
        .size(44.0)
        .background(Background::fill(c))
        .show(ui);
}

/// Pill-shaped tag chip — the chip look IS the demo content here, so it
/// carries its own fill and stroke. Both are translucent accent, so the
/// cloud harmonizes with the rest of the page.
fn chip<H: Hash>(ui: &mut Ui, key: H, label: &'static str) {
    let a = support::A;
    Panel::hstack()
        .id_salt(("chip-row", &key))
        .padding((10.0, 4.0))
        .background(
            Background::rounded(a.with_alpha(0.20), Corners::all(10.0))
                .with_stroke(Stroke::solid(a.with_alpha(0.45), 1.0)),
        )
        .show(ui, |ui| {
            Text::new(label)
                .id_salt(("chip-label", &key))
                .style(&TextStyle::default().with_font_size(12.0))
                .show(ui);
        });
}

fn badge<H: Hash>(ui: &mut Ui, key: H, w: f32) {
    let size = (Sizing::fixed(w), Sizing::fixed(24.0));
    support::swatch(ui, ("badge", &key), size, support::A);
}

#[track_caller]
fn grid_tile(
    ui: &mut Ui,
    label: &'static str,
    cell: (u16, u16),
    span: Option<(u16, u16)>,
    size: Option<(Sizing, Sizing)>,
    color: RgbaF32,
) {
    let mut tile = Panel::zstack()
        .auto_id()
        .padding(5.0)
        .grid_cell(match span {
            Some((rows, cols)) => GridCell::at(cell.0, cell.1).span(rows, cols),
            None => cell.into(),
        })
        .background(swatch_bg(color));
    if let Some(sz) = size {
        tile = tile.size(sz);
    }
    tile.show(ui, |ui| {
        Text::new(label)
            .style(&on_swatch_style().with_font_size(11.0))
            .show(ui);
    });
}
