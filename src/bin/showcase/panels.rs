//! Container drivers on one page: the four stack panels (HStack /
//! VStack / ZStack / Canvas), flow-then-wrap WrapHStack / WrapVStack
//! (`.gap` spaces siblings within a line, `.line_gap` spaces lines,
//! `.justify` applies per-line), and Grid with fixed / fill / hug /
//! clamped tracks plus cell spanning.

use crate::support;
use crate::support::{demo_cell, on_swatch_text, section, swatch_bg};
use aperture::{
    Background, Color, Configure, Corners, Frame, Grid, Justify, Panel, Sizing, Stroke, Text,
    TextStyle, Track, Ui,
};
use std::hash::Hash;

pub(crate) fn build(ui: &mut Ui) {
    support::page(ui, |ui| {
        support::header(
            ui,
            "Container drivers — stacks, wrapping flow, and Grid tracks. \
             Resize the window to watch wrap lines and Fill tracks re-divide.",
        );

        Panel::hstack()
            .id_salt("stacks-row")
            .gap(12.0)
            .size((Sizing::FILL, Sizing::Fixed(170.0)))
            .show(ui, |ui| {
                demo_cell(ui, "HStack", |ui| {
                    Panel::hstack().auto_id().gap(6.0).show(ui, |ui| {
                        sw(ui, "h-a", 40.0, 40.0, support::A);
                        sw(ui, "h-b", 40.0, 40.0, support::A);
                        sw(ui, "h-c", 40.0, 40.0, support::A);
                    });
                });
                demo_cell(ui, "VStack", |ui| {
                    Panel::vstack().auto_id().gap(6.0).show(ui, |ui| {
                        sw(ui, "v-a", 60.0, 24.0, support::A);
                        sw(ui, "v-b", 60.0, 24.0, support::A);
                        sw(ui, "v-c", 60.0, 24.0, support::A);
                    });
                });
                demo_cell(ui, "ZStack", |ui| {
                    Panel::zstack().auto_id().show(ui, |ui| {
                        sw(ui, "z-back", 80.0, 80.0, support::A);
                        sw(ui, "z-front", 50.0, 50.0, support::B);
                    });
                });
                demo_cell(ui, "Canvas — positioned children", |ui| {
                    Panel::canvas()
                        .auto_id()
                        .size((Sizing::FILL, Sizing::FILL))
                        .show(ui, |ui| {
                            positioned(ui, "p1", 10.0, 10.0, support::A);
                            positioned(ui, "p2", 60.0, 30.0, support::B);
                            positioned(ui, "p3", 30.0, 70.0, support::C);
                        });
                });
            });

        section(
            ui,
            "wrap-h",
            "WrapHStack — tag cloud flows along the main axis, wraps when the next chip wouldn't fit",
            |ui| {
                Panel::wrap_hstack()
                    .id_salt("tags")
                    .size((Sizing::FILL, Sizing::Hug))
                    .gap(8.0)
                    .line_gap(8.0)
                    .show(ui, |ui| {
                        for (i, t) in TAGS.iter().enumerate() {
                            chip(ui, ("tag", i), t);
                        }
                    });
            },
        );

        Panel::hstack()
            .id_salt("wrap-row")
            .gap(24.0)
            .size((Sizing::FILL, Sizing::Hug))
            .show(ui, |ui| {
                section(
                    ui,
                    "wrap-v",
                    "WrapVStack — vertical flow, wraps to a new column",
                    |ui| {
                        Panel::wrap_vstack()
                            .id_salt("vwrap-col")
                            .size((Sizing::Hug, Sizing::Fixed(130.0)))
                            .gap(6.0)
                            .line_gap(12.0)
                            .show(ui, |ui| {
                                for i in 0..10 {
                                    badge(ui, ("v", i));
                                }
                            });
                    },
                );
                section(
                    ui,
                    "wrap-c",
                    "WrapHStack — per-line Justify::Center",
                    |ui| {
                        Panel::wrap_hstack()
                            .id_salt("centered-row")
                            .size((Sizing::FILL, Sizing::Hug))
                            .gap(10.0)
                            .line_gap(10.0)
                            .justify(Justify::Center)
                            .show(ui, |ui| {
                                for i in 0..7 {
                                    badge(ui, ("badge", i));
                                }
                            });
                    },
                );
            });

        Panel::hstack()
            .id_salt("grid-row")
            .gap(12.0)
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                // Classic three-column app shell: fixed sidebar | flexible
                // content | hugging right rail; the header spans all three.
                demo_cell(
                    ui,
                    "Grid — app shell (fixed | fill | hug, header spans)",
                    |ui| {
                        Grid::new()
                            .id_salt("shell-grid")
                            .cols([Track::fixed(140.0), Track::fill(), Track::hug()])
                            .rows([Track::fixed(36.0), Track::fill()])
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
                                    Some((Sizing::Fixed(80.0), Sizing::FILL)),
                                    support::D,
                                );
                            });
                    },
                );
                // The left Fill is bounded [200, 300] so it grows with the
                // window only within that range; the right Fill absorbs
                // every leftover pixel. Resize to watch the sidebar saturate.
                demo_cell(
                    ui,
                    "Grid — clamped track (Fill min 200 max 300 | Fill)",
                    |ui| {
                        Grid::new()
                            .id_salt("clamped")
                            .cols([
                                Track::fill_weight(1.0).min(200.0).max(300.0),
                                Track::fill_weight(2.0),
                            ])
                            .rows([Track::fill()])
                            .gap(8.0)
                            .size((Sizing::FILL, Sizing::FILL))
                            .show(ui, |ui| {
                                grid_tile(ui, "sidebar", (0, 0), None, None, support::A);
                                grid_tile(ui, "main", (0, 1), None, None, support::D);
                            });
                    },
                );
            });
    });
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

fn sw(ui: &mut Ui, id: &'static str, w: f32, h: f32, c: Color) {
    Frame::new()
        .id_salt(id)
        .size((Sizing::Fixed(w), Sizing::Fixed(h)))
        .background(swatch_bg(c))
        .show(ui);
}

fn positioned(ui: &mut Ui, id: &'static str, x: f32, y: f32, c: Color) {
    Frame::new()
        .id_salt(id)
        .position((x, y))
        .size(40.0)
        .background(Background {
            fill: c.into(),
            ..Default::default()
        })
        .show(ui);
}

/// Pill-shaped tag chip — the "chip" look IS the demo aesthetic, so a
/// bg + stroke is needed to make it read as a chip rather than bare
/// text. Uses a translucent accent so chips harmonize with the palette.
fn chip<H: Hash>(ui: &mut Ui, key: H, label: &'static str) {
    let a = support::A;
    Panel::hstack()
        .id_salt(("chip-row", &key))
        .padding((10.0, 4.0))
        .background(Background {
            fill: Color::linear_rgba(a.r, a.g, a.b, 0.20).into(),
            stroke: Stroke::solid(Color::linear_rgba(a.r, a.g, a.b, 0.45), 1.0),
            corners: Corners::all(10.0),
            ..Default::default()
        })
        .show(ui, |ui| {
            Text::new(label)
                .id_salt(("chip-label", &key))
                .style(TextStyle::default().with_font_size(12.0))
                .show(ui);
        });
}

fn badge<H: Hash>(ui: &mut Ui, key: H) {
    Frame::new()
        .id_salt(("badge", &key))
        .size((Sizing::Fixed(80.0), Sizing::Fixed(28.0)))
        .background(swatch_bg(support::A))
        .show(ui);
}

fn grid_tile(
    ui: &mut Ui,
    label: &'static str,
    cell: (u16, u16),
    span: Option<(u16, u16)>,
    size: Option<(Sizing, Sizing)>,
    color: Color,
) {
    let mut tile = Panel::zstack()
        .id_salt(label)
        .padding(6.0)
        .grid_cell(cell)
        .background(swatch_bg(color));
    if let Some(s) = span {
        tile = tile.grid_span(s);
    }
    if let Some(sz) = size {
        tile = tile.size(sz);
    }
    tile.show(ui, |ui| {
        Text::new(label)
            .id_salt((label, "tile-label"))
            .style(on_swatch_text().with_font_size(11.0))
            .show(ui);
    });
}
